//! Dead/unreachable code corrections and conversion semantics, oracle checked.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{get_range, quickfix_client_capabilities, QuickFixTest};
use serde_json::{json, Value};
const INCLUDING: &str = "Remove (including condition)";
fn setup(source: &str) -> (QuickFixTest, String) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.deadCode".into(),
        "warning".into(),
    );
    let root = t.ws.new_empty_project(&options);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    (t, uri)
}
fn action(t: &mut QuickFixTest, uri: &str, title: &str) -> Value {
    let actions = t.evaluate_code_actions(uri);
    actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| {
            panic!(
                "Missing {title}: {actions:#?}; diagnostics={:#?}",
                t.diagnostics(uri)
            )
        })
        .clone()
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
#[test]
fn numeric_widening_keeps_the_conditional_type() {
    let s = run(
        "package p; public class A { long value() {return true ? 1 : 2L;} }",
        INCLUDING,
    );
    contains(&s, "return (long) 1;");
}
#[test]
fn boxed_reference_cast_keeps_numeric_boxing() {
    let s = run(
        "package p; public class A { Object value() {return true ? 1 : (Object) \"s\";} }",
        INCLUDING,
    );
    contains(&s, "return (Object) 1;");
}
#[test]
fn narrowing_unboxed_branch_is_explicit() {
    let s=run("package p; public class A { Object value() {return true ? new Integer(1) + 2 : new Double(0.0) + 3;} }",INCLUDING);
    contains(&s, "return (double) (new Integer(1) + 2);");
}
#[test]
fn overloaded_call_keeps_the_original_object_overload() {
    let s=run("package p; public class A { void use(Object value) {} void use(String value) {} void run() {use(false ? new Object() : \"s\");} }",INCLUDING);
    contains(&s, "use((Object) \"s\");");
}
#[test]
fn external_overloaded_call_keeps_the_original_signature() {
    let s=run("package p; public class A { void run() {System.out.println(false ? new Object() : \"s\");} }",INCLUDING);
    contains(&s, "System.out.println((Object) \"s\");");
}
#[test]
fn reference_assignment_without_overloads_needs_no_cast() {
    let s = run(
        "package p; public class A { Object value() {return false ? new Object() : \"s\";} }",
        INCLUDING,
    );
    contains(&s, "return \"s\";");
    assert!(!s.contains("(Object)"), "{s}");
}
#[test]
fn lambda_keeps_its_variable_target_without_a_cast() {
    let s = run(
        "package p; public class A { Runnable value = true ? () -> {} : () -> {}; }",
        INCLUDING,
    );
    contains(&s, "Runnable value = () -> {};");
}
#[test]
fn method_reference_keeps_its_variable_target_without_a_cast() {
    let s=run("package p; public class A { Runnable value = true ? this::run : this::run; void run() {} }",INCLUDING);
    contains(&s, "Runnable value = this::run;");
}
#[test]
fn conditional_raw_conversion_is_retained() {
    let s=run("package p; public class A { java.util.List<String> value() {return true ? new java.util.ArrayList() : null;} }",INCLUDING);
    contains(&s, "return (List<String>) new java.util.ArrayList();");
    assert!(s.contains("import java.util.List;"), "{s}");
}
#[test]
fn empty_replacement_preserves_an_enclosing_control_body() {
    let s=run("package p; public class A { void run(boolean b) {if (b) if (false) work();} void work() {} }",INCLUDING);
    contains(&s, "if (b) {}");
}
#[test]
fn false_for_body_is_promoted_like_the_reference() {
    let s = run(
        "package p; public class A { void run() {for (;false;) work();} void work() {} }",
        INCLUDING,
    );
    contains(&s, "void run() {work();}");
}
#[test]
fn trailing_statements_stop_at_the_next_switch_case() {
    let s=run("package p; public class A { void run(int i) {switch(i) {case 1: return; work(); work(); case 2: work(); break; default: break;}} void work() {} }","Remove");
    contains(&s, "case 1: return; case 2: work(); break;");
}
#[test]
fn switch_expression_trailing_statements_are_removed() {
    let s=run("package p; public class A { int run(int i) {return switch(i) {case 1 -> {yield 3; work();} default -> 4;};} void work() {} }","Remove");
    contains(&s, "case 1 -> {yield 3;}");
}
#[test]
fn split_or_copies_then_and_moves_the_else_branch() {
    let s=run("package p; public class A { void run(boolean b) {if (true || b) yes(); else no();} void yes() {} void no() {} }","Split || condition");
    contains(&s, "if (true) yes(); else if (b) yes(); else no();");
}
#[test]
fn split_and_copies_else_into_both_branches() {
    let s=run("package p; public class A { void run(boolean b) {if (false && b) yes(); else no();} void yes() {} void no() {} }","Split && condition");
    contains(&s, "if (false) { if (b) yes(); else no(); } else no();");
}
#[test]
fn parenthesized_condition_does_not_offer_a_split() {
    let (mut t,uri)=setup("package p; public class A { void run(boolean b) {if ((false && b)) yes();} void yes() {} }");
    let actions = t.evaluate_code_actions(&uri);
    assert!(
        !actions.iter().any(|a| a["title"] == "Split && condition"),
        "{actions:#?}"
    );
}
#[test]
fn promoted_block_keeps_comments_and_blank_lines() {
    let source="package p;\npublic class A {\n    void run() {\n        if (false) {\n            dead();\n        } else {\n            // first\n            live();\n\n            // second\n            live();\n        }\n    }\n    void dead() {}\n    void live() {}\n}\n";
    let expected="package p;\npublic class A {\n    void run() {\n        // first\n        live();\n\n        // second\n        live();\n    }\n    void dead() {}\n    void live() {}\n}\n";
    assert_eq!(run(source, INCLUDING), expected);
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
fn deferred_removal_keeps_diagnostics_and_resource_edit_shape() {
    let source = "package p; public class A { void run() {if(false) dead();} void dead() {} }";
    let (mut t, uri) = setup(source);
    t.ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    t.ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        json!(["create", "rename", "delete"]);
    let a = action(&mut t, &uri, INCLUDING);
    assert!(a["edit"].is_null(), "{a}");
    assert!(!a["diagnostics"].as_array().unwrap().is_empty());
    let resolved = t.ws.request("codeAction/resolve", a);
    assert!(resolved["edit"]["documentChanges"].is_array(), "{resolved}");
    contains(
        &apply_edits(source, edits(&resolved["edit"], &uri)),
        "void run() {}",
    );
}
#[test]
fn open_buffer_unicode_crlf_removal_leaves_disk_unchanged() {
    let disk = "package p; public class A {}";
    let source="package p;\r\n// 😀 café\r\npublic class A {\r\n    void run() {\r\n        if (false) dead();\r\n    }\r\n    void dead() {}\r\n}\r\n";
    let (mut t, uri) = setup(disk);
    t.diagnostics(&uri);
    t.ws.change(&uri, source);
    let a = action(&mut t, &uri, INCLUDING);
    let s = apply_edits(source, edits(&a["edit"], &uri));
    contains(&s, "// 😀 café");
    assert!(!s.contains("if (false)"), "{s}");
    assert!(!s.replace("\r\n", "").contains('\n'), "{s:?}");
    assert_eq!(t.ws.read(&uri), disk);
}
#[test]
fn virtual_documents_offer_removals_from_real_diagnostics() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Dead.java",
        "inmemory://parity/Dead.java",
        "file:///tmp/jdtls-parity-missing-dead/A.java",
    ] {
        let source = "class A { void run() {if(false) dead();} void dead() {} }";
        let mut ws = Workspace::new();
        ws.capabilities = quickfix_client_capabilities();
        ws.init_options["compilerOptions"] = json!({"org.eclipse.jdt.core.compiler.problem.deadCode":"warning", "org.eclipse.jdt.core.compiler.problem.deadCodeInTrivialIfStatement":"disabled"});
        ws.open_with(uri, source);
        let diagnostics = ws.diagnostics(uri);
        let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,"dead();"),"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
        let a = actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == INCLUDING)
            .unwrap_or_else(|| panic!("{actions:#?}"));
        contains(
            &apply_edits(source, edits(&a["edit"], uri)),
            "void run() {}",
        );
        assert!(!actions
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["title"] == "Remove unreachable code"));
    }
}
#[test]
fn inherited_overload_keeps_original_parameter_type() {
    let s=run("package p; public class A extends B { void use(String value) {} void run() {use(false ? new Object() : \"s\");} } class B { void use(Object value) {} }",INCLUDING);
    contains(&s, "use((Object) \"s\");");
}
#[test]
fn overloaded_constructor_keeps_original_parameter_type() {
    let s=run("package p; public class A { A(Object value) {} A(String value) {} static void run() {new A(false ? new Object() : \"s\");} }",INCLUDING);
    contains(&s, "new A((Object) \"s\");");
}
#[test]
fn overloaded_lambda_keeps_the_functional_interface_cast() {
    let s=run("package p; public class A { void use(I value) {} void use(J value) {} void run() {use(true ? () -> {} : (I) () -> {});} } interface I {void run();} interface J {void run();}",INCLUDING);
    contains(&s, "use((I) () -> {});");
}
#[test]
fn overloaded_method_reference_keeps_the_functional_interface_cast() {
    let s=run("package p; public class A { void use(I value) {} void use(J value) {} void work() {} void run() {use(true ? this::work : (I) this::work);} } interface I {void run();} interface J {void run();}",INCLUDING);
    contains(&s, "use((I) this::work);");
}
#[test]
fn functional_overloads_with_different_void_compatibility_need_no_cast() {
    let s=run("package p; public class A { void use(java.util.function.Supplier<String> value) {} void use(Runnable value) {} void run() {use(true ? () -> \"s\" : () -> \"t\");} }",INCLUDING);
    contains(&s, "use(() -> \"s\");");
}
#[test]
fn preserved_condition_side_effects_match_reference_removal_rule() {
    let s=run("package p; public class A { void run() {if (probe() || true) live(); else dead();} boolean probe(){return false;} void live() {} void dead() {} }",INCLUDING);
    contains(&s, "void run() {live();}");
}
