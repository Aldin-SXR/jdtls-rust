//! Unused declarations: binding identity, evaluation, calls and working copies.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{get_range, quickfix_client_capabilities, QuickFixTest};
use serde_json::{json, Value};
fn setup(source: &str) -> (QuickFixTest, String) {
    setup_options(source, &[])
}
fn setup_options(source: &str, overrides: &[(&str, &str)]) -> (QuickFixTest, String) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for key in [
        "unusedLocal",
        "unusedParameter",
        "unusedPrivateMember",
        "unusedTypeParameter",
    ] {
        options.insert(
            format!("org.eclipse.jdt.core.compiler.problem.{key}"),
            "warning".into(),
        );
    }
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    for (key, value) in overrides {
        options.insert((*key).into(), (*value).into());
    }
    let root = t.ws.new_empty_project(&options);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    (t, uri)
}
fn action(t: &mut QuickFixTest, uri: &str, title: &str) -> Value {
    let actions = t.evaluate_code_actions(uri);
    select_action(&actions, title)
}
fn select_action(actions: &[Value], title: &str) -> Value {
    assert!(
        !actions.iter().any(|a| matches!(
            a["title"].as_str(),
            Some("Remove unused member" | "Remove unused variable")
        )),
        "{actions:#?}"
    );
    actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("Missing {title}: {actions:#?}"))
        .clone()
}
fn keep(name: &str) -> String {
    format!("Remove '{name}', keep assignments with side effects")
}
fn force(name: &str) -> String {
    format!("Remove '{name}' and all assignments")
}
fn run(source: &str, title: &str) -> String {
    let (mut t, uri) = setup(source);
    let a = action(&mut t, &uri, title);
    assert_eq!(a["kind"], "quickfix");
    t.evaluate_code_action_command(&a)
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, fragment: &str) {
    assert!(
        compact(s).contains(&compact(fragment)),
        "Missing {fragment}: {s}"
    );
}
fn edits<'a>(edit: &'a Value, uri: &str) -> &'a Vec<Value> {
    edit["changes"][uri]
        .as_array()
        .or_else(|| {
            edit["documentChanges"]
                .as_array()
                .and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri))
                .and_then(|c| c["edits"].as_array())
        })
        .unwrap_or_else(|| panic!("{edit:#?}"))
}
#[test]
fn field_removal_does_not_touch_a_shadowing_local_or_other_class() {
    let s=run("package p; public class A { private int value; void run(){this.value=3; int value=4; System.out.println(value);} } class B {int value; void run(){value=5;}}", &keep("value"));
    contains(&s, "void run(){int value=4; System.out.println(value);}");
    contains(&s, "class B {int value; void run(){value=5;}}");
    assert!(!s.contains("private int value"), "{s}");
}
#[test]
fn initializer_effects_remain_in_evaluation_order() {
    let s=run("package p; public class A { void run(){int unused=compute(1)+compute(2);} int compute(int value){return value;} }", &keep("unused"));
    contains(&s, "void run(){compute(1); compute(2);}");
}
#[test]
fn nested_invocation_is_kept_as_one_effect() {
    let s=run("package p; public class A { void run(){int unused=compute(compute(1));} int compute(int value){return value;} }", &keep("unused"));
    contains(&s, "void run(){compute(compute(1));}");
    assert_eq!(s.matches("compute(1)").count(), 1, "{s}");
}
#[test]
fn constructor_initializer_is_kept_for_a_local() {
    let s = run(
        "package p; public class A { void run(){Object unused=new Object();} }",
        &keep("unused"),
    );
    contains(&s, "void run(){new Object();}");
}
#[test]
fn field_initializer_is_discarded_like_the_reference() {
    let s = run(
        "package p; public class A {private Object unused=new Object();}",
        &keep("unused"),
    );
    contains(&s, "public class A {}");
}
#[test]
fn forced_removal_drops_initializer_and_assignment_calls() {
    let s=run("package p; public class A { void run(){int unused=compute(); unused=compute();} int compute(){return 1;} }", &force("unused"));
    contains(&s, "void run(){}");
}
#[test]
fn chained_assignment_keeps_the_surviving_assignment() {
    let source="package p; public class A { int other; void run(){int unused; unused=other=compute();} int compute(){return 1;} }";
    for label in [keep("unused"), force("unused")] {
        contains(&run(source, &label), "void run(){other=compute();}");
    }
}
#[test]
fn assignment_control_body_is_replaced_with_an_empty_block() {
    let s = run(
        "package p; public class A {void run(boolean b){int unused=0; if(b) unused++;}}",
        &keep("unused"),
    );
    contains(&s, "void run(boolean b){if(b){}}");
}
#[test]
fn right_hand_field_access_preserves_its_receiver_call() {
    let s=run("package p; public class A {int value; void run(){int unused=0; unused=make().value;} A make(){return this;}}",&keep("unused"));
    contains(&s, "void run(){make();}");
}
#[test]
fn for_initializer_becomes_ordered_expressions() {
    let s=run("package p; public class A {void run(boolean b){for(int unused=compute(1)+compute(2);b;) {break;}} int compute(int i){return i;}}",&keep("unused"));
    contains(&s, "for(compute(1), compute(2);b;)");
}
#[test]
fn effectful_multiple_for_initializer_has_no_keep_edit() {
    let (mut t,uri)=setup("package p; public class A {void run(boolean b){for(int unused=compute(), used=0;b;used++) {System.out.println(used);}} int compute(){return 1;}}");
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions.iter().any(|a| a["title"] == keep("unused")),
        "{actions:#?}"
    );
    let a = select_action(&actions, &force("unused"));
    contains(
        &t.evaluate_code_action_command(&a),
        "for(int used=0;b;used++)",
    );
}
#[test]
fn conditional_initializer_preserves_branch_evaluation() {
    let s=run("package p; public class A {void run(boolean b){int unused=b?compute(1):compute(2);} int compute(int i){return i;}}",&keep("unused"));
    contains(
        &s,
        "void run(boolean b){if(b){compute(1);}else{compute(2);}}",
    );
}
#[test]
fn conditional_constructor_branches_follow_the_reference_node_rule() {
    let s=run("package p; public class A {void run(boolean b){Object unused=b?new Object():new Object();}}",&keep("unused"));
    contains(&s, "void run(boolean b){}");
}
#[test]
fn split_first_fragment_preserves_following_declarations() {
    let s=run("package p; public class A {void run(){int unused=compute(), used=1; System.out.println(used);} int compute(){return 1;}}",&keep("unused"));
    contains(
        &s,
        "void run(){compute(); int used=1; System.out.println(used);}",
    );
}
#[test]
fn parameter_removal_updates_bound_calls_and_preserves_an_overload() {
    let s=run("package p; public class A {private void use(int unused, String used){System.out.println(used);} void use(String other){System.out.println(other);} void run(){use(1,\"a\"); use(\"b\");}}","Remove unused parameter 'unused'");
    contains(&s, "private void use1(String used)");
    contains(&s, "void run(){use1(\"a\"); use(\"b\");}");
}
#[test]
fn parameter_collision_checks_inherited_methods_and_suffixes() {
    let s=run("package p; public class A extends B {private void use(int unused, String used){System.out.println(used);} void run(){use(1,\"a\");}} class B {public void use(String s){System.out.println(s);} public void use1(String s){System.out.println(s);}}","Remove unused parameter 'unused'");
    contains(&s, "private void use2(String used)");
    contains(&s, "void run(){use2(\"a\");}");
}
#[test]
fn method_reference_suppresses_parameter_removal() {
    let (mut t,uri)=setup("package p; public class A {private void use(int unused){} void run(){java.util.function.IntConsumer consumer=this::use; consumer.accept(1);}}");
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions
            .iter()
            .any(|a| a["title"] == "Remove unused parameter 'unused'"),
        "{actions:#?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| a["title"] == "Document parameter to avoid 'unused' warning"),
        "{actions:#?}"
    );
}
#[test]
fn parameter_removal_drops_its_javadoc_tag() {
    let source="package p; public class A {\n    /**\n     * Description.\n     * @param unused old\n     * @param used kept\n     */\n    private void use(int unused, int used){System.out.println(used);}\n    void run(){use(1,2);}\n}";
    let (mut t, uri) = setup(source);
    let project = t.ws.roots[0].clone();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedParameter".into(),
        "warning".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedParameterIncludeDocCommentReference".into(),
        "disabled".into(),
    );
    t.ws.set_project_options(&project, &options);
    let a = action(&mut t, &uri, "Remove unused parameter 'unused'");
    let s = t.evaluate_code_action_command(&a);
    contains(&s, "private void use(int used)");
    assert!(s.contains("@param used kept"), "{s}");
    assert!(!s.contains("@param unused"), "{s}");
}
#[test]
fn deferred_removal_has_diagnostics_and_resource_edits() {
    let source = "package p; public class A {private int unused;}";
    let (mut t, uri) = setup(source);
    t.ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    t.ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        json!(["create", "rename", "delete"]);
    let a = action(&mut t, &uri, &keep("unused"));
    assert!(a["edit"].is_null(), "{a}");
    assert!(!a["diagnostics"].as_array().unwrap().is_empty());
    let resolved = t.ws.request("codeAction/resolve", a);
    assert!(resolved["edit"]["documentChanges"].is_array(), "{resolved}");
    contains(
        &apply_edits(source, edits(&resolved["edit"], &uri)),
        "public class A {}",
    );
}
#[test]
fn unsaved_unicode_crlf_buffer_does_not_modify_disk() {
    let disk = "package p; public class A {}";
    let source="package p;\r\n// 😀 café\r\npublic class A {\r\n    void run(){\r\n        int unused=compute();\r\n    }\r\n    int compute(){return 1;}\r\n}\r\n";
    let (mut t, uri) = setup(disk);
    t.diagnostics(&uri);
    t.ws.change(&uri, source);
    let a = action(&mut t, &uri, &keep("unused"));
    let s = apply_edits(source, edits(&a["edit"], &uri));
    contains(&s, "void run(){compute();}");
    assert!(s.contains("// 😀 café"), "{s}");
    assert!(!s.replace("\r\n", "").contains('\n'), "{s:?}");
    assert_eq!(t.ws.read(&uri), disk);
}
#[test]
fn virtual_documents_remove_unused_fields_with_real_diagnostics() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Unused.java",
        "inmemory://parity/Unused.java",
        "file:///tmp/jdtls-parity-missing-unused/A.java",
    ] {
        let source = "class A {private int unused;}";
        let mut ws = Workspace::new();
        ws.capabilities = quickfix_client_capabilities();
        ws.init_options["compilerOptions"] =
            json!({"org.eclipse.jdt.core.compiler.problem.unusedPrivateMember":"warning"});
        ws.open_with(uri, source);
        let diagnostics = ws.diagnostics(uri);
        let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,"unused"),"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
        let a = actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == keep("unused"))
            .unwrap_or_else(|| panic!("{actions:#?}"));
        contains(&apply_edits(source, edits(&a["edit"], uri)), "class A {}");
    }
}

#[test]
fn unused_class_type_parameter_can_be_removed() {
    let s = run(
        "package p; public class A<T> {}",
        "Remove unused type parameter",
    );
    contains(&s, "public class A {}");
}
#[test]
fn unused_method_type_parameter_can_be_removed() {
    let s = run(
        "package p; public class A { public <T> void run(){} }",
        "Remove unused type parameter",
    );
    contains(&s, "public void run(){}");
}
#[test]
fn unused_type_parameter_can_be_documented() {
    let s = run(
        "package p; public class A<T> {}",
        "Document type parameter to avoid 'unused' warning",
    );
    contains(&s, "/** * @param <T> */ public class A<T> {}");
}
#[test]
fn parameter_documentation_is_inserted_in_declaration_order() {
    let s=run("package p; public class A {\n    /**\n     * Description.\n     * @param used kept\n     */\n    private void run(int unused, int used){System.out.println(used);}\n    public void call(){run(1,2);}\n}", "Document parameter to avoid 'unused' warning");
    let first = s.find("@param unused").unwrap_or_else(|| panic!("{s}"));
    let second = s.find("@param used kept").unwrap_or_else(|| panic!("{s}"));
    assert!(first < second, "{s}");
    assert!(s.contains("Description."), "{s}");
}
#[test]
fn disabled_doc_support_suppresses_documentation_actions() {
    let (mut t, uri) = setup_options(
        "package p; public class A {private void run(int unused){} public void call(){run(1);}}",
        &[(
            "org.eclipse.jdt.core.compiler.doc.comment.support",
            "disabled",
        )],
    );
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions
            .iter()
            .any(|a| a["title"] == "Document parameter to avoid 'unused' warning"),
        "{actions:#?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| a["title"] == "Remove unused parameter 'unused'"),
        "{actions:#?}"
    );
}
#[test]
fn java22_for_initializer_can_be_renamed_to_unnamed() {
    let (mut t, uri) = setup_options(
        "package p; public class A {void run(boolean b){for(int unused=1;b;) {break;}}}",
        &[
            ("org.eclipse.jdt.core.compiler.source", "22"),
            ("org.eclipse.jdt.core.compiler.compliance", "22"),
            ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "22"),
        ],
    );
    let a = action(&mut t, &uri, "Rename to unnamed variable");
    contains(&t.evaluate_code_action_command(&a), "for(int _=1;b;)");
}
#[test]
fn java22_enhanced_for_offers_rename_without_removal() {
    let (mut t, uri) = setup_options(
        "package p; public class A {void run(int[] values){for(int unused:values) {}}}",
        &[
            ("org.eclipse.jdt.core.compiler.source", "22"),
            ("org.eclipse.jdt.core.compiler.compliance", "22"),
            ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "22"),
        ],
    );
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions
            .iter()
            .any(|a| a["title"] == keep("unused") || a["title"] == force("unused")),
        "{actions:#?}"
    );
    let a = select_action(&actions, "Rename to unnamed variable");
    contains(&t.evaluate_code_action_command(&a), "for(int _:values)");
}

#[test]
fn java22_implicit_lambda_parameter_can_be_unnamed() {
    let (mut t,uri)=setup_options("package p; public class A {void run(){java.util.function.IntUnaryOperator f=(unused)->1; System.out.println(f.applyAsInt(2));}}", &[
        ("org.eclipse.jdt.core.compiler.source","22"),("org.eclipse.jdt.core.compiler.compliance","22"),("org.eclipse.jdt.core.compiler.codegen.targetPlatform","22")
    ]);
    let a = action(&mut t, &uri, "Rename to unnamed variable");
    contains(&t.evaluate_code_action_command(&a), "f=(_)->1");
}
#[test]
fn java21_lambda_parameter_cannot_be_unnamed() {
    let (mut t,uri)=setup("package p; public class A {void run(){java.util.function.IntUnaryOperator f=(unused)->1; System.out.println(f.applyAsInt(2));}}");
    assert!(!t
        .evaluate_code_actions(&uri)
        .iter()
        .any(|a| a["title"] == "Rename to unnamed variable"));
}
#[test]
fn typed_java22_lambda_retains_the_reference_rename_restriction() {
    let (mut t,uri)=setup_options("package p; public class A {void run(){java.util.function.IntUnaryOperator f=(int unused)->1; System.out.println(f.applyAsInt(2));}}", &[
        ("org.eclipse.jdt.core.compiler.source","22"),("org.eclipse.jdt.core.compiler.compliance","22"),("org.eclipse.jdt.core.compiler.codegen.targetPlatform","22")
    ]);
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions
            .iter()
            .any(|a| a["title"] == "Rename to unnamed variable"),
        "{actions:#?}"
    );
}

#[test]
fn type_parameter_documentation_follows_existing_type_tags() {
    let s = run(
        "package p;\n/**\n * Description.\n * @param <T> kept\n */\npublic class A<T, U> {}",
        "Document type parameter to avoid 'unused' warning",
    );
    let first = s.find("@param <T> kept").unwrap_or_else(|| panic!("{s}"));
    let second = s.find("@param <U>").unwrap_or_else(|| panic!("{s}"));
    assert!(first < second, "{s}");
}
