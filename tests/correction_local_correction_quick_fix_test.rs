//! Selected LocalCorrectionQuickFixTest ports with original fixtures and full-source assertions.
mod common;
use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(),
        "error".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.deadCode".into(),
        "warning".into(),
    );
    let root = t.ws.new_empty_project(&options);
    t.set_ignored_commands(&["Extract.*"]);
    t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        "package test1;\npublic interface E {\n    void foo();\n}\n",
    );
    (t, root)
}
#[test]
fn test_unimplemented_methods() {
    let (mut t, root) = setup();
    let uri = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "F.java",
        "package test1;\npublic class F implements E {\n}\n",
    );
    t.assert_code_actions(&uri,&[Expected::new("Add unimplemented methods","package test1;\npublic class F implements E {\n\n    @Override\n    public void foo() {\n        // TODO Auto-generated method stub\n        throw new UnsupportedOperationException(\"Unimplemented method 'foo'\");\n    }\n}\n")]);
}
#[test]
fn test_unimplemented_methods_for_enum() {
    let (mut t, root) = setup();
    let uri = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "F.java",
        "package test1;\npublic enum F implements E {\n}\n",
    );
    t.assert_code_actions(&uri,&[Expected::new("Add unimplemented methods","package test1;\npublic enum F implements E {\n    ;\n\n    @Override\n    public void foo() {\n        // TODO Auto-generated method stub\n        throw new UnsupportedOperationException(\"Unimplemented method 'foo'\");\n    }\n}\n")]);
}

fn dead_setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(),
        "error".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.deadCode".into(),
        "warning".into(),
    );
    let root = t.ws.new_empty_project(&options);
    t.set_ignored_commands(&["Extract.*"]);
    (t, root)
}

#[test]
fn test_remove_unreachable_code_stmt() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unnecessaryElse".into(),
        "ignore".into(),
    );
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(int x) {\n        if (x == 9) {\n            return true;\n        } else\n            return false;\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(int x) {\n        if (x == 9) {\n            return true;\n        } else\n            return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_unreachable_code_stmt2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test", "E.java", "package test;\npublic class E {\n    public String getName() {\n        try{\n            return \"fred\";\n        }\n        catch (Exception e){\n            return e.getLocalizedMessage();\n        }\n        System.err.print(\"wow\");\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test;\npublic class E {\n    public String getName() {\n        try{\n            return \"fred\";\n        }\n        catch (Exception e){\n            return e.getLocalizedMessage();\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_unreachable_code_while() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo() {\n        while (false) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public boolean foo() {\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        if (false) {\n            System.out.println(\"a\");\n        } else {\n            System.out.println(\"b\");\n        }\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        System.out.println(\"b\");\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) {\n            if (o == null) {\n            \tSystem.out.println(\"hello\");\n        \t} else {\n            \tSystem.out.println(\"bye\");\n        \t}\n        }\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) {\n            System.out.println(\"bye\");\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then3() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) \n            if (o == null) {\n            \tSystem.out.println(\"hello\");\n        \t} else {\n            \tSystem.out.println(\"bye\");\n            \tSystem.out.println(\"bye-bye\");\n        \t}\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) {\n        \tSystem.out.println(\"bye\");\n        \tSystem.out.println(\"bye-bye\");\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then4() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) \n            if (true) \n            \tif (o == null) \n            \t\tSystem.out.println(\"hello\");\n\t\tSystem.out.println(\"bye\");\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) \n            if (true) {\n            }\n\t\tSystem.out.println(\"bye\");\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then5() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) \n            if (false) \n            \tif (o == null) \n            \t\tSystem.out.println(\"hello\");\n\t\tSystem.out.println(\"bye\");\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = new Object();\n        if (o != null) {\n        }\n\t\tSystem.out.println(\"bye\");\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_then_switch() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        switch (1) {\n            case 1:\n                if (false) {\n                \tfoo();\n\t\t\t\t\tSystem.out.println(\"hi\");\n\t\t\t\t} else {\n                \tSystem.out.println(\"bye\");\n\t\t\t\t}\n                break;\n            case 2:\n                foo();\n                break;\n            default:\n                break;\n        };\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        switch (1) {\n            case 1:\n            System.out.println(\"bye\");\n                break;\n            case 2:\n                foo();\n                break;\n            default:\n                break;\n        };\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_if_else() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        if (Math.random() == -1 || true) {\n            System.out.println(\"a\");\n        } else {\n            System.out.println(\"b\");\n        }\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        System.out.println(\"a\");\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo() {\n        if (true) return false;\n        return true;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public boolean foo() {\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((false && b1) && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if3() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((b1 && false) && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (b1 && false) {\n            return true;\n        }\n        return false;\n    }\n}\n"),
        Expected::new("Split && condition", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (b1 && false) {\n            if (b2) {\n                return true;\n            }\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if4() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((((b1 && false))) && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (b1 && false) {\n            return true;\n        }\n        return false;\n    }\n}\n"),
        Expected::new("Split && condition", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (b1 && false) {\n            if (b2) {\n                return true;\n            }\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if5() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((((b1 && false) && b2))) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (b1 && false) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if6() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((((false && b1) && b2))) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (((false && b2))) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if7() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((((false && b1))) && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if8() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1) {\n        if ((((false && b1)))) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1) {\n        if (false) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if9() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false && b1 && b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false) {\n            return true;\n        }\n        return false;\n    }\n}\n"),
        Expected::new("Split && condition", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false) {\n            if (b1 && b2) {\n                return true;\n            }\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if10() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (((false && b1 && b2))) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if (false) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if11() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1) {\n        if ((true || b1) && false) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1) {\n        if (true && false) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if12() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2, boolean b3) {\n        if (((b1 && false) && b2) | b3) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2, boolean b3) {\n        if ((b1 && false) | b3) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_after_if13() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((false | false && b1) & b2) {\n            return true;\n        }\n        return false;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove", "package test1;\npublic class E {\n    public boolean foo(boolean b1, boolean b2) {\n        if ((false | false) & b2) {\n            return true;\n        }\n        return false;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_conditional() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public int foo() {\n        return true ? 1 : 0;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public int foo() {\n        return 1;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_conditional2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = true ? new Integer(1) + 2 : new Double(0.0) + 3;\n        System.out.println(o);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = (double) (new Integer(1) + 2);\n        System.out.println(o);\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_conditional3() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        Object o = true ? new Integer(1) : new Double(0.0);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        Object o = (double) new Integer(1);\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_dead_code_multi_statements() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        if (true)\n            return;\n        foo();\n        foo();\n        foo();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove (including condition)", "package test1;\npublic class E {\n    public void foo() {\n        return;\n    }\n}\n")
    ]);
}

#[test]
fn test_remove_unreachable_code_multi_statements_switch() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        switch (1) {\n        case 1:\n            foo();\n            break;\n            foo();\n            new Object();\n        case 2:\n            foo();\n            break;\n        default:\n            break;\n        };\n    }\n}\n");
    t.assert_code_action_exists_expected(&uri, &Expected::new("Remove", "package test1;\npublic class E {\n    public void foo() {\n        switch (1) {\n        case 1:\n            foo();\n            break;\n        case 2:\n            foo();\n            break;\n        default:\n            break;\n        };\n    }\n}\n"));
}

#[test]
fn test_unused_private_field() {
    unused_private_field(false);
}
fn unused_private_field(resource_support: bool) {
    let (mut t, root) = dead_setup();
    if resource_support { t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] = serde_json::json!(["create", "rename", "delete"]); }
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private int count;\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove 'count', keep assignments with side effects", "package test1;\npublic class E {\n}\n"),
        Expected::new("Generate Getter and Setter for 'count'", "package test1;\npublic class E {\n    private int count;\n\n    /**\n     * @return the count\n     */\n    public int getCount() {\n        return count;\n    }\n\n    /**\n     * @param count the count to set\n     */\n    public void setCount(int count) {\n        this.count = count;\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_private_field_with_resource_operation_support() {
    unused_private_field(true);
}

#[test]
fn test_unused_private_field1() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private int count, color= count;\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove 'color', keep assignments with side effects", "package test1;\npublic class E {\n    private int count;\n}\n"),
        Expected::new("Generate Getter and Setter for 'color'", "package test1;\npublic class E {\n    private int count, color= count;\n\n    /**\n     * @return the color\n     */\n    public int getColor() {\n        return color;\n    }\n\n    /**\n     * @param color the color to set\n     */\n    public void setColor(int color) {\n        this.color = color;\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_private_field2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private int count= 0;\n    public void foo() {\n        count= 1 + 2;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove 'count', keep assignments with side effects", "package test1;\npublic class E {\n    public void foo() {\n    }\n}\n"),
        Expected::new("Generate Getter and Setter for 'count'", "package test1;\npublic class E {\n    private int count= 0;\n    /**\n     * @return the count\n     */\n    public int getCount() {\n        return count;\n    }\n    /**\n     * @param count the count to set\n     */\n    public void setCount(int count) {\n        this.count = count;\n    }\n    public void foo() {\n        count= 1 + 2;\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_parameter() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedParameter".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private void foo(int i, int j) {\n       System.out.println(j);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove unused parameter 'i'", "package test1;\npublic class E {\n    private void foo(int j) {\n       System.out.println(j);\n    }\n}\n"),
        Expected::new("Document parameter to avoid 'unused' warning", "package test1;\npublic class E {\n    /**\n     * @param i  \n     */\n    private void foo(int i, int j) {\n       System.out.println(j);\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_method() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private void foo() {}\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove method 'foo'", "package test1;\npublic class E {\n}\n")
    ]);
}

#[test]
fn test_unused_private_constructor() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    int i;\n    private E() {}\n    public E(int i) {\n        this.i = i;    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove constructor 'E'", "package test1;\npublic class E {\n    int i;\n    public E(int i) {\n        this.i = i;    }\n}\n")
    ]);
}

#[test]
fn test_unused_local_variable() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedLocal".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void foo() {\n        int i = 0;\n        i++;\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove 'i' and all assignments", "package test1;\npublic class E {\n    public void foo() {\n    }\n}\n"),
        Expected::new("Remove 'i', keep assignments with side effects", "package test1;\npublic class E {\n    public void foo() {\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_local_variable_with_keeping_assignments() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedLocal".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "B.java", "package test1;\npublic class B {\n    void test(){\n        String c=\"Test\",d=String.valueOf(true),e=c;\n        e+=\"\";\n        d=\"blubb\";\n        d=String.valueOf(12);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove 'd' and all assignments", "package test1;\npublic class B {\n    void test(){\n        String c=\"Test\",e=c;\n        e+=\"\";\n    }\n}\n"),
        Expected::new("Remove 'd', keep assignments with side effects", "package test1;\npublic class B {\n    void test(){\n        String c=\"Test\";\n        String.valueOf(true);\n        String e=c;\n        e+=\"\";\n        String.valueOf(12);\n    }\n}\n")
    ]);
}

#[test]
fn test_unused_type_parameter() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedTypeParameter".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    private static class Foo {}\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Remove type 'Foo'", "package test1;\npublic class E {\n}\n")
    ]);
}

#[test]
fn test_unneeded_catch_block() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n        } catch (ParseException e) {\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove catch clause","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n        }\n    }\n}\n"),
        Expected::new("Replace catch clause with throws","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() throws ParseException {\n        try {\n            goo();\n        } catch (IOException e) {\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_unneeded_catch_block_in_initializer() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.text.ParseException;\npublic class E {\n    static {\n        try {\n            int x= 1;\n        } catch (ParseException e) {\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove catch clause","package test1;\nimport java.text.ParseException;\npublic class E {\n    static {\n        int x= 1;\n    }\n}\n")
    ]);
}

#[test]
fn test_unneeded_catch_block_single() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove catch clause","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() {\n        goo();\n    }\n}\n"),
        Expected::new("Replace catch clause with throws","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() throws IOException {\n        goo();\n    }\n}\n")
    ]);
}

#[test]
fn test_unneeded_catch_block_with_finally() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n        } finally {\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove catch clause","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() {\n        try {\n            goo();\n        } finally {\n        }\n    }\n}\n"),
        Expected::new("Replace catch clause with throws","package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() {\n    }\n    public void foo() throws IOException {\n        try {\n            goo();\n        } finally {\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_unnecessary_thrown_exception1() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\npublic class E {\n    public void foo(String b) throws IOException {\n        if  (b != null) {\n            System.out.println();\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove thrown exception","package test1;\n\npublic class E {\n    public void foo(String b) {\n        if  (b != null) {\n            System.out.println();\n        }\n    }\n}\n"),
        Expected::new("Document thrown exception to avoid 'unused' warning","package test1;\nimport java.io.IOException;\npublic class E {\n    /**\n\t * @throws IOException  \n\t */\n    public void foo(String b) throws IOException {\n        if  (b != null) {\n            System.out.println();\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_unnecessary_thrown_exception2() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @throws IOException\n     */\n    public E(int i) throws IOException, ParseException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove thrown exception","package test1;\nimport java.io.IOException;\npublic class E {\n    /**\n     * @throws IOException\n     */\n    public E(int i) throws IOException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n}\n"),
        Expected::new("Document thrown exception to avoid 'unused' warning","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @throws IOException\n     * @throws ParseException \n     */\n    public E(int i) throws IOException, ParseException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_unnecessary_thrown_exception3() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionIncludeDocCommentReference".into(), "disabled".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @param i\n     * @throws IOException\n     * @throws ParseException\n     */\n    public void foo(int i) throws IOException, ParseException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove thrown exception","package test1;\nimport java.io.IOException;\npublic class E {\n    /**\n     * @param i\n     * @throws IOException\n     */\n    public void foo(int i) throws IOException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_unnecessary_thrown_exception4() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root,"src","test1","E.java","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @throws IOException\n     */\n    public E(int i) throws IOException, ParseException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n    public void foo(int i) throws ParseException {\n        if  (i == 0) {\n            throw new ParseException(null, 4);\n        }\n    }\n}\n");
    t.assert_code_actions(&uri,&[
        Expected::new("Remove thrown exception","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @throws IOException\n     */\n    public E(int i) throws IOException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n    public void foo(int i) throws ParseException {\n        if  (i == 0) {\n            throw new ParseException(null, 4);\n        }\n    }\n}\n"),
        Expected::new("Document thrown exception to avoid 'unused' warning","package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    /**\n     * @throws IOException\n     * @throws ParseException \n     */\n    public E(int i) throws IOException, ParseException {\n        if  (i == 0) {\n            throw new IOException();\n        }\n    }\n    public void foo(int i) throws ParseException {\n        if  (i == 0) {\n            throw new ParseException(null, 4);\n        }\n    }\n}\n")
    ]);
}

#[test]
fn test_expression_should_be_variable() {
    let (mut t, root) = dead_setup();
    let before = "package test1;\npublic class E {\n    public static void foo (String input) {\n        ((String)input);\n    }\n}";
    let after = "package test1;\npublic class E {\n    public static void foo (String input) {\n        String string = (String)input;\n    }\n}";
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", before);
    t.assert_code_action_exists_expected(&uri, &Expected::new("Create local variable using expression", after));
}

#[test]
fn test_set_parenteses1() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.localVariableHiding".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.fieldHiding".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let before = "package test1;\npublic class E {\n    public void foo(Object x) {\n        if (!x instanceof Runnable) {\n        }\n    }\n}\n";
    let after = "package test1;\npublic class E {\n    public void foo(Object x) {\n        if (!(x instanceof Runnable)) {\n        }\n    }\n}\n";
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", before);
    t.assert_code_actions(&uri, &[Expected::new("Put 'instanceof' in parentheses", after)]);
}

#[test]
fn test_set_parenteses2() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.localVariableHiding".into(), "error".into());
    options.insert("org.eclipse.jdt.core.compiler.problem.fieldHiding".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let before = "package test1;\npublic class E {\n    public boolean foo(int x) {\n        return !x instanceof Runnable || true;\n    }\n}\n";
    let after = "package test1;\npublic class E {\n    public boolean foo(int x) {\n        return !(x instanceof Runnable) || true;\n    }\n}\n";
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", before);
    t.assert_code_actions(&uri, &[Expected::new("Put 'instanceof' in parentheses", after)]);
}

#[test]
fn test_unnecessary_nls_tag() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.nonExternalizedStringLiteral".into(), "error".into());
    options.insert("org.eclipse.jdt.core.formatter.tabulation.char".into(), "tab".into());
    t.ws.set_project_options(&root, &options);
    let before = "package test1;\npublic class E {\n\tpublic void foo(int count) {\n\t\tint a = count; //$NON-NLS-1$\n\t}\n}\n";
    let after = "package test1;\npublic class E {\n\tpublic void foo(int count) {\n\t\tint a = count;\n\t}\n}\n";
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", before);
    t.assert_code_action_exists_expected(&uri, &Expected::new("Remove unnecessary '$NON-NLS$' tag", after));
}


// Uncaught-exception ports: original upstream inputs and full-source expectations.
#[test]
fn test_uncaught_exception() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() {\n        goo();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() throws IOException {\n        goo();\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() throws IOException {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\npublic class E {\n    public String goo() throws IOException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     */\n    public void foo() {\n        goo().substring(2);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\npublic class E {\n    public String goo() throws IOException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     * @throws IOException \n     */\n    public void foo() throws IOException {\n        goo().substring(2);\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\npublic class E {\n    public String goo() throws IOException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     */\n    public void foo() {\n        try {\n            goo().substring(2);\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception3() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public String goo() throws IOException, ParseException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     * @throws ParseException Parsing failed\n     */\n    public void foo() throws ParseException {\n        goo().substring(2);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public String goo() throws IOException, ParseException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     * @throws ParseException Parsing failed\n     * @throws IOException \n     */\n    public void foo() throws ParseException, IOException {\n        goo().substring(2);\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public String goo() throws IOException, ParseException {\n        return null;\n    }\n    /**\n     * Not much to say here.\n     * @throws ParseException Parsing failed\n     */\n    public void foo() throws ParseException {\n        try {\n            goo().substring(2);\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        } catch (ParseException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception4() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.FileNotFoundException;\nimport java.io.InterruptedIOException;\npublic class E {\n    public E goo(int i) throws InterruptedIOException {\n        return new E();\n    }\n    public E bar() throws FileNotFoundException {\n        return new E();\n    }\n    /**\n     * Not much to say here.\n     */\n    public void foo() {\n        goo(1).bar();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.FileNotFoundException;\nimport java.io.InterruptedIOException;\npublic class E {\n    public E goo(int i) throws InterruptedIOException {\n        return new E();\n    }\n    public E bar() throws FileNotFoundException {\n        return new E();\n    }\n    /**\n     * Not much to say here.\n     * @throws InterruptedIOException \n     * @throws FileNotFoundException \n     */\n    public void foo() throws FileNotFoundException, InterruptedIOException {\n        goo(1).bar();\n    }\n}\n"),
        Expected::new("Surround with try/multi-catch", "package test1;\nimport java.io.FileNotFoundException;\nimport java.io.InterruptedIOException;\npublic class E {\n    public E goo(int i) throws InterruptedIOException {\n        return new E();\n    }\n    public E bar() throws FileNotFoundException {\n        return new E();\n    }\n    /**\n     * Not much to say here.\n     */\n    public void foo() {\n        try {\n            goo(1).bar();\n        } catch (FileNotFoundException | InterruptedIOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.FileNotFoundException;\nimport java.io.InterruptedIOException;\npublic class E {\n    public E goo(int i) throws InterruptedIOException {\n        return new E();\n    }\n    public E bar() throws FileNotFoundException {\n        return new E();\n    }\n    /**\n     * Not much to say here.\n     */\n    public void foo() {\n        try {\n            goo(1).bar();\n        } catch (FileNotFoundException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        } catch (InterruptedIOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception5() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void throwIOException () throws IOException {\n        throw new IOException();\n    }\n    void foo() {\n        try {\n            throwIOException();\n        } catch (IOException e) {\n            throwIOException();\n        }\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void throwIOException () throws IOException {\n        throw new IOException();\n    }\n    void foo() throws IOException {\n        try {\n            throwIOException();\n        } catch (IOException e) {\n            throwIOException();\n        }\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void throwIOException () throws IOException {\n        throw new IOException();\n    }\n    void foo() {\n        try {\n            throwIOException();\n        } catch (IOException e) {\n            try {\n                throwIOException();\n            } catch (IOException e1) {\n                // TODO Auto-generated catch block\n                e1.printStackTrace();\n            }\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_import_conflict() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "Test.java", "package test1;\npublic class Test {\n    public void test1() {\n        test2();\n    }\n\n    public void test2() throws de.muenchen.test.Exception {\n        throw new de.muenchen.test.Exception();\n    }\n\n    public void test3() {\n        try {\n            java.io.File.createTempFile(\"\", \".tmp\");\n        } catch (Exception ex) {\n\n        }\n    }\n}\n");
    t.ws.create_cu(&root, "src", "de.muenchen.test", "Exception.java", "package de.muenchen.test;\n\npublic class Exception extends java.lang.Throwable {\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\npublic class Test {\n    public void test1() throws de.muenchen.test.Exception {\n        test2();\n    }\n\n    public void test2() throws de.muenchen.test.Exception {\n        throw new de.muenchen.test.Exception();\n    }\n\n    public void test3() {\n        try {\n            java.io.File.createTempFile(\"\", \".tmp\");\n        } catch (Exception ex) {\n\n        }\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\npublic class Test {\n    public void test1() {\n        try {\n            test2();\n        } catch (de.muenchen.test.Exception e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n\n    public void test2() throws de.muenchen.test.Exception {\n        throw new de.muenchen.test.Exception();\n    }\n\n    public void test3() {\n        try {\n            java.io.File.createTempFile(\"\", \".tmp\");\n        } catch (Exception ex) {\n\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_remove_more_specific() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\nimport java.net.SocketException;\npublic class E {\n    public void goo() throws IOException {\n        return;\n    }\n    /**\n     * @throws SocketException Sockets are dangerous\n     * @since 3.0\n     */\n    public void foo() throws SocketException {\n        this.goo();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\npublic class E {\n    public void goo() throws IOException {\n        return;\n    }\n    /**\n     * @throws IOException \n     * @since 3.0\n     */\n    public void foo() throws IOException {\n        this.goo();\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\nimport java.net.SocketException;\npublic class E {\n    public void goo() throws IOException {\n        return;\n    }\n    /**\n     * @throws SocketException Sockets are dangerous\n     * @since 3.0\n     */\n    public void foo() throws SocketException {\n        try {\n            this.goo();\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_to_surrounding_try() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public static void goo() throws IOException, ParseException {\n        return;\n    }\n    public void foo() {\n        try {\n            E.goo();\n        } catch (IOException e) {\n        }\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public static void goo() throws IOException, ParseException {\n        return;\n    }\n    public void foo() throws ParseException {\n        try {\n            E.goo();\n        } catch (IOException e) {\n        }\n    }\n}\n"),
        Expected::new("Add catch clause to surrounding try", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public static void goo() throws IOException, ParseException {\n        return;\n    }\n    public void foo() {\n        try {\n            E.goo();\n        } catch (IOException e) {\n        } catch (ParseException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
        Expected::new("Add exception to existing catch clause", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public static void goo() throws IOException, ParseException {\n        return;\n    }\n    public void foo() {\n        try {\n            E.goo();\n        } catch (IOException | ParseException e) {\n        }\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public static void goo() throws IOException, ParseException {\n        return;\n    }\n    public void foo() {\n        try {\n            try {\n                E.goo();\n            } catch (ParseException e) {\n                // TODO Auto-generated catch block\n                e.printStackTrace();\n            }\n        } catch (IOException e) {\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_bug2711() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n    public void throwException () throws Exception {\n        throw new Exception();\n    }    public void test () {\n        throwException();\n        try {\n        } catch (Exception e) {\n            // TODO: handle exception\n        }\n    }\n}");
    t.assert_code_actions(&uri, &[
        Expected::new("Surround with try/catch", "package test1;\npublic class E {\n    public void throwException () throws Exception {\n        throw new Exception();\n    }    public void test () {\n        try {\n            throwException();\n        } catch (Exception e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n        try {\n        } catch (Exception e) {\n            // TODO: handle exception\n        }\n    }\n}"),
    ]);
}

#[test]
fn test_multi_catch_uncaught_exceptions() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.EOFException;\nimport java.io.FileNotFoundException;\npublic class E {\n    public void foo() throws EOFException {}\n    public void bar() throws FileNotFoundException {}\n    public void test() {\n        System.out.println(1);\n        foo();\n        System.out.println(2);\n        bar();\n        System.out.println(3);\n    }\n}\n");
    t.assert_code_actions_range(&uri, serde_json::json!({"start": {"line": 7, "character": 8}, "end": {"line": 11, "character": 30}}), &[
        Expected::new("Surround with try/multi-catch", "package test1;\nimport java.io.EOFException;\nimport java.io.FileNotFoundException;\npublic class E {\n    public void foo() throws EOFException {}\n    public void bar() throws FileNotFoundException {}\n    public void test() {\n        try {\n            System.out.println(1);\n            foo();\n            System.out.println(2);\n            bar();\n            System.out.println(3);\n        } catch (EOFException | FileNotFoundException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_multi_catch_uncaught_exceptions2() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.EOFException;\nimport java.io.FileNotFoundException;\npublic class E {\n    public void foo() throws EOFException {}\n    public void bar() throws FileNotFoundException {}\n    public void test() {\n        System.out.println(1);\n        foo();\n        System.out.println(2);\n        bar();\n        System.out.println(3);\n    }\n}\n");
    t.assert_code_actions_range(&uri, serde_json::json!({"start": {"line": 8, "character": 8}, "end": {"line": 10, "character": 14}}), &[
        Expected::new("Surround with try/multi-catch", "package test1;\nimport java.io.EOFException;\nimport java.io.FileNotFoundException;\npublic class E {\n    public void foo() throws EOFException {}\n    public void bar() throws FileNotFoundException {}\n    public void test() {\n        System.out.println(1);\n        try {\n            foo();\n            System.out.println(2);\n            bar();\n        } catch (EOFException | FileNotFoundException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n        System.out.println(3);\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super1() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.FileInputStream;\npublic class E extends FileInputStream {\n    public E() {\n        super(\"x\");\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.FileInputStream;\nimport java.io.FileNotFoundException;\npublic class E extends FileInputStream {\n    public E() throws FileNotFoundException {\n        super(\"x\");\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super2() {
    let (mut t, root) = dead_setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    public A() throws Exception {\n    }\n}\n");
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E extends A {\n    /**\n     * @throws Exception sometimes...\n     */\n    public E() {\n        super();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\npublic class E extends A {\n    /**\n     * @throws Exception sometimes...\n     */\n    public E() throws Exception {\n        super();\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super3() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A implements Runnable {\n    public void run() {\n        Class.forName(null);\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Surround with try/catch", "package test1;\npublic class A implements Runnable {\n    public void run() {\n        try {\n            Class.forName(null);\n        } catch (ClassNotFoundException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super4() {
    let (mut t, root) = dead_setup();
    t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\npublic class A {\n    public void foo() {\n    }\n}\n");
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E extends A {\n    private void throwException() throws Exception {\n        throw new Exception();\n    }\n    public void foo() {\n        throwException();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\npublic class E extends A {\n    private void throwException() throws Exception {\n        throw new Exception();\n    }\n    public void foo() throws Exception {\n        throwException();\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\npublic class E extends A {\n    private void throwException() throws Exception {\n        throw new Exception();\n    }\n    public void foo() {\n        try {\n            throwException();\n        } catch (Exception e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super5() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\nimport java.io.Closeable;\nimport java.io.FileNotFoundException;\npublic class A implements Closeable {\n    public void throwFileNotFoundException () throws FileNotFoundException {\n        throw new FileNotFoundException();\n    }\n    public void close() {\n        throwFileNotFoundException();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.Closeable;\nimport java.io.FileNotFoundException;\npublic class A implements Closeable {\n    public void throwFileNotFoundException () throws FileNotFoundException {\n        throw new FileNotFoundException();\n    }\n    public void close() throws FileNotFoundException {\n        throwFileNotFoundException();\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.Closeable;\nimport java.io.FileNotFoundException;\npublic class A implements Closeable {\n    public void throwFileNotFoundException () throws FileNotFoundException {\n        throw new FileNotFoundException();\n    }\n    public void close() {\n        try {\n            throwFileNotFoundException();\n        } catch (FileNotFoundException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_on_super6() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "A.java", "package test1;\nimport java.io.Closeable;\npublic class A implements Closeable {\n    public void throwThrowable() throws Throwable {\n        throw new Throwable();\n    }\n    public void close() {\n        throwThrowable();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.Closeable;\npublic class A implements Closeable {\n    public void throwThrowable() throws Throwable {\n        throw new Throwable();\n    }\n    public void close() {\n        try {\n            throwThrowable();\n        } catch (Throwable e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_duplicate() {
    let (mut t, root) = dead_setup();
    t.ws.create_cu(&root, "src", "test1", "MyException.java", "package test1;\npublic class MyException extends Exception {\n}\n");
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void m1() throws IOException {\n        m2();\n    }\n    public void m2() throws IOException, ParseException, MyException {\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void m1() throws IOException, ParseException, MyException {\n        m2();\n    }\n    public void m2() throws IOException, ParseException, MyException {\n    }\n}\n"),
        Expected::new("Surround with try/multi-catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void m1() throws IOException {\n        try {\n            m2();\n        } catch (IOException | ParseException | MyException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n    public void m2() throws IOException, ParseException, MyException {\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void m1() throws IOException {\n        try {\n            m2();\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        } catch (ParseException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        } catch (MyException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n    public void m2() throws IOException, ParseException, MyException {\n    }\n}\n"),
    ]);
}

#[test]
fn test_multiple_uncaught_exceptions() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException, ParseException {\n    }\n    public void foo() {\n        goo();\n    }\n}\n");
    t.assert_code_actions(&uri, &[
        Expected::new("Add throws declaration", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException, ParseException {\n    }\n    public void foo() throws IOException, ParseException {\n        goo();\n    }\n}\n"),
        Expected::new("Surround with try/multi-catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException, ParseException {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException | ParseException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
        Expected::new("Surround with try/catch", "package test1;\nimport java.io.IOException;\nimport java.text.ParseException;\npublic class E {\n    public void goo() throws IOException, ParseException {\n    }\n    public void foo() {\n        try {\n            goo();\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        } catch (ParseException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}\n"),
    ]);
}

#[test]
fn test_uncaught_exception_for_closeable() {
    let (mut t, root) = dead_setup();
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\n\nimport java.io.FileInputStream;\nimport java.io.InputStream;\nimport java.nio.file.Path;\n\npublic class E {\n    public void test () {\n        InputStream inp = new FileInputStream(Path.of(\"test\").toFile());\n    }\n}");
    t.assert_code_actions(&uri, &[
        Expected::new("Surround with try-with-resources", "package test1;\n\nimport java.io.FileInputStream;\nimport java.io.IOException;\nimport java.io.InputStream;\nimport java.nio.file.Path;\n\npublic class E {\n    public void test () {\n        try (InputStream inp = new FileInputStream(Path.of(\"test\").toFile())) {\n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        }\n    }\n}"),
    ]);
}

#[test]
fn test_unused_allocation1() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n\tpublic void foo(int count) throws Exception {\n\t\tnew RuntimeException();\n\t}\n}\n");
    t.assert_code_action_exists_expected(&uri, &Expected::new("Throw the allocated object", "package test1;\npublic class E {\n\tpublic void foo(int count) throws Exception {\n\t\tthrow new RuntimeException();\n\t}\n}\n"));
}

#[test]
fn test_unused_allocation2() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n\tpublic String foo(int count) throws Exception {\n\t\tif (count < 3) {\n\t\t\tnew String(\"abc\");\n\t\t}\n\t\treturn \"def\";\n\t}\n}\n");
    t.assert_code_action_exists_expected(&uri, &Expected::new("Return the allocated object", "package test1;\npublic class E {\n\tpublic String foo(int count) throws Exception {\n\t\tif (count < 3) {\n\t\t\treturn new String(\"abc\");\n\t\t}\n\t\treturn \"def\";\n\t}\n}\n"));
}

#[test]
fn test_unused_allocation3() {
    let (mut t, root) = dead_setup();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(), "error".into());
    t.ws.set_project_options(&root, &options);
    let uri = t.ws.create_cu(&root, "src", "test1", "E.java", "package test1;\npublic class E {\n\tpublic String foo(int count) throws Exception {\n\t\tif (count < 3) {\n\t\t\tnew String(\"abc\");\n\t\t}\n\t\treturn \"def\";\n\t}\n}\n");
    t.assert_code_action_exists_expected(&uri, &Expected::new("Remove", "package test1;\npublic class E {\n\tpublic String foo(int count) throws Exception {\n\t\tif (count < 3) {\n\t\t}\n\t\treturn \"def\";\n\t}\n}\n"));
}
