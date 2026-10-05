//! Catch removal/conversion and unused throws, checked against the Java server.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{get_range, quickfix_client_capabilities, QuickFixTest};
use serde_json::{json, Value};
const REMOVE: &str = "Remove catch clause";
const THROWS: &str = "Replace catch clause with throws";
const UNUSED: &str = "Remove thrown exception";
fn setup(source: &str) -> (QuickFixTest, String) {
    setup_options(source, &[])
}
fn setup_options(source: &str, overrides: &[(&str, &str)]) -> (QuickFixTest, String) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException".into(),
        "warning".into(),
    );
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    for (k, v) in overrides {
        options.insert((*k).into(), (*v).into());
    }
    let root = t.ws.new_empty_project(&options);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    (t, uri)
}
fn action(t: &mut QuickFixTest, uri: &str, title: &str) -> Value {
    let actions = t.evaluate_code_actions(uri);
    assert!(
        !actions
            .iter()
            .any(|a| a["title"] == "Remove unused thrown exception"),
        "{actions:#?}"
    );
    actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("Missing {title}: {actions:#?}"))
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
fn contains(s: &str, f: &str) {
    assert!(compact(s).contains(&compact(f)), "Missing {f}: {s}");
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
fn multiple_body_statements_are_wrapped_in_a_control_body() {
    let s=run("package p; public class A {void run(boolean b){if(b) try {work(); other();}catch(java.io.IOException e){}} void work(){} void other(){}}",REMOVE);
    contains(&s, "if(b) {work(); other();}");
}
#[test]
fn single_body_statement_is_promoted_without_braces() {
    let s=run("package p; public class A {void run(boolean b){if(b) try {work();}catch(java.io.IOException e){}} void work(){}}",REMOVE);
    contains(&s, "if(b) work();");
}
#[test]
fn empty_try_is_removed() {
    let s = run(
        "package p; public class A {void run(){try {}catch(java.io.IOException e){}}}",
        REMOVE,
    );
    contains(&s, "void run(){}");
}
#[test]
fn copied_body_preserves_comments_and_blank_lines() {
    let source="package p;\npublic class A {\n    void run() {\n        try {\n            work();\n\n            // keep this gap\n            other();\n        } catch (java.io.IOException e) {\n        }\n    }\n    void work() {}\n    void other() {}\n}\n";
    let expected="package p;\npublic class A {\n    void run() {\n        work();\n\n        // keep this gap\n        other();\n    }\n    void work() {}\n    void other() {}\n}\n";
    assert_eq!(run(source, REMOVE), expected);
}
#[test]
fn removing_a_catch_preserves_resources() {
    let s=run("package p; public class A {void run(){try(R r=new R()){work();}catch(java.text.ParseException e){}} void work(){}} class R implements AutoCloseable {public void close(){}}",REMOVE);
    contains(&s, "try(R r=new R()){work();}");
}
#[test]
fn removing_a_catch_preserves_finally_evaluation() {
    let s=run("package p; public class A {void run(){try{work();}catch(java.io.IOException e){dead();}finally{done();}} void work(){} void dead(){} void done(){}}",REMOVE);
    contains(&s, "void run(){try{work();}finally{done();}}");
}
#[test]
fn multi_catch_selected_exception_is_removed() {
    let s=run("package p; public class A {void run(){try{}catch(java.io.IOException | java.text.ParseException e){}}}","Remove exception");
    contains(&s, "catch(java.text.ParseException e)");
}
#[test]
fn multi_catch_selected_exception_becomes_a_throws_type() {
    let s=run("package p; public class A {void run(){try{}catch(java.io.IOException | java.text.ParseException e){}}}","Replace exception with throws");
    contains(&s, "void run()throws java.io.IOException");
    contains(&s, "catch(java.text.ParseException e)");
}
#[test]
fn invalid_union_subtype_alternative_is_removed() {
    let s = run(
        "package p; public class A {void run(){try{}catch(java.io.IOException | Exception e){}}}",
        "Remove exception",
    );
    contains(&s, "catch(Exception e)");
}
#[test]
fn existing_supertype_throws_suppresses_a_new_exception() {
    let s=run("package p; public class A {void run()throws Exception {try{}catch(java.io.IOException e){}}}",THROWS);
    contains(&s, "void run()throws Exception {}");
    assert!(!s.contains("IOException"), "{s}");
}
#[test]
fn existing_same_throws_type_is_not_duplicated() {
    let s=run("package p; public class A {void run()throws java.io.IOException {try{}catch(java.io.IOException e){}}}",THROWS);
    contains(&s, "void run()throws java.io.IOException {}");
    assert_eq!(s.matches("IOException").count(), 1, "{s}");
}
#[test]
fn catch_to_throws_retains_the_reference_override_behavior() {
    let s=run("package p; public class A implements I {public void run(){try{}catch(java.io.IOException e){}}} interface I {void run();}",THROWS);
    contains(&s, "public void run()throws java.io.IOException {}");
}
#[test]
fn qualified_annotated_catch_type_is_copied_to_throws() {
    let s=run("package p; public class A {void run(){try{}catch(java.io.@Flag IOException e){}}} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}",THROWS);
    contains(&s, "void run()throws java.io.@Flag IOException {}");
}
#[test]
fn unused_throws_removes_its_only_import() {
    let s = run(
        "package p;\nimport java.io.IOException;\npublic class A {void run()throws IOException {}}",
        UNUSED,
    );
    contains(&s, "void run(){}");
    assert!(!s.contains("import java.io.IOException"), "{s}");
}
#[test]
fn unused_throws_keeps_imports_used_by_a_field_or_array() {
    let s=run("package p;\nimport java.io.IOException;\npublic class A {IOException[] errors=new IOException[1]; void run()throws IOException {}}",UNUSED);
    assert!(s.contains("import java.io.IOException;"), "{s}");
    contains(&s, "void run(){}");
}
#[test]
fn unused_throws_keeps_imports_used_in_type_arguments() {
    let s=run("package p;\nimport java.io.IOException;\npublic class A {java.util.List<IOException> errors; void run()throws IOException {}}",UNUSED);
    assert!(s.contains("import java.io.IOException;"), "{s}");
    contains(&s, "void run(){}");
}
#[test]
fn unused_throws_type_literal_follows_the_reference_counter() {
    let s=run("package p;\nimport java.io.IOException;\npublic class A {Class<?> type=IOException.class; void run()throws IOException {}}",UNUSED);
    assert!(!s.contains("import java.io.IOException;"), "{s}");
    contains(&s, "Class<?> type=IOException.class");
}
#[test]
fn unused_throws_removes_the_matching_exception_tag() {
    let (mut t,uri)=setup_options("package p;\nimport java.io.IOException;\npublic class A {\n    /**\n     * Description.\n     * @exception java.io.IOException no longer needed\n     */\n    void run()throws IOException {}\n}", &[("org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownExceptionIncludeDocCommentReference","disabled")]);
    let a = action(&mut t, &uri, UNUSED);
    let s = t.evaluate_code_action_command(&a);
    assert!(!s.contains("@exception"), "{s}");
    assert!(!s.contains("import java.io.IOException;"), "{s}");
    assert!(s.contains("Description."), "{s}");
}
#[test]
fn unused_annotated_qualified_throws_documentation_omits_annotations() {
    let s=run("package p; public class A {void run()throws java.io.@Flag IOException {}} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}", "Document thrown exception to avoid 'unused' warning");
    contains(&s, "@throws java.io.IOException");
    contains(&s, "throws java.io.@Flag IOException");
}
#[test]
fn deferred_catch_removal_keeps_diagnostics_and_resource_edits() {
    let source = "package p; public class A {void run(){try{}catch(java.io.IOException e){}}}";
    let (mut t, uri) = setup(source);
    t.ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    t.ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        json!(["create", "rename", "delete"]);
    let a = action(&mut t, &uri, REMOVE);
    assert!(a["edit"].is_null(), "{a}");
    assert!(!a["diagnostics"].as_array().unwrap().is_empty());
    let resolved = t.ws.request("codeAction/resolve", a);
    assert!(resolved["edit"]["documentChanges"].is_array(), "{resolved}");
    contains(
        &apply_edits(source, edits(&resolved["edit"], &uri)),
        "void run(){}",
    );
}
#[test]
fn unsaved_unicode_crlf_catch_removal_leaves_disk_unchanged() {
    let disk = "package p; public class A {}";
    let source="package p;\r\n// 😀 café\r\npublic class A {\r\n    void run(){\r\n        try{work();}catch(java.io.IOException e){}\r\n    }\r\n    void work(){}\r\n}\r\n";
    let (mut t, uri) = setup(disk);
    t.diagnostics(&uri);
    t.ws.change(&uri, source);
    let a = action(&mut t, &uri, REMOVE);
    let s = apply_edits(source, edits(&a["edit"], &uri));
    contains(&s, "void run(){work();}");
    assert!(s.contains("// 😀 café"), "{s}");
    assert!(!s.replace("\r\n", "").contains('\n'), "{s:?}");
    assert_eq!(t.ws.read(&uri), disk);
}
#[test]
fn virtual_documents_offer_catch_and_throws_corrections() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Exceptions.java",
        "inmemory://parity/Exceptions.java",
        "file:///tmp/jdtls-parity-missing-exceptions/A.java",
    ] {
        for (source, selection, title, expected) in [
            (
                "class A {void run(){try{}catch(java.io.IOException e){}}}",
                "java.io.IOException",
                REMOVE,
                "void run(){}",
            ),
            (
                "class A {void run()throws java.io.IOException {}}",
                "java.io.IOException",
                UNUSED,
                "void run(){}",
            ),
        ] {
            let mut ws = Workspace::new();
            ws.capabilities = quickfix_client_capabilities();
            ws.init_options["compilerOptions"] = json!({"org.eclipse.jdt.core.compiler.problem.unusedDeclaredThrownException":"warning"});
            ws.open_with(uri, source);
            let diagnostics = ws.diagnostics(uri);
            let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,selection),"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
            let a = actions
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["title"] == title)
                .unwrap_or_else(|| panic!("{actions:#?}"));
            contains(&apply_edits(source, edits(&a["edit"], uri)), expected);
        }
    }
}
