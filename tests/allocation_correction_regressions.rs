//! Unused allocation options and AssignToVariableAssistProposalCore, via LSP.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::quickfix_client_capabilities;
use common::quickfix::{get_range, get_title, QuickFixTest};
use serde_json::json;

fn run(name: &str, source: &str, selection: &str) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(),
        "error".into(),
    );
    if name == "local-affix" {
        options.insert(
            "org.eclipse.jdt.core.codeComplete.localPrefixes".into(),
            "m".into(),
        );
    }
    let root = t.ws.new_empty_project(&options);
    if name == "field-this" {
        std::fs::write(
            root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
            "eclipse.preferences.version=1\norg.eclipse.jdt.ui.keywordthis=true\n",
        )
        .unwrap();
    }
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    let actions = t.evaluate_code_actions_range(&uri, get_range(source, selection));
    assert!(
        !actions.iter().any(|a| matches!(
            get_title(a).as_str(),
            "Assign to new local variable" | "Assign to new field"
        )),
        "legacy approximate assignment actions: {actions:#?}"
    );
    if name.starts_with("no-") {
        assert!(
            !actions.iter().any(|a| a["kind"] == "quickfix"
                && (get_title(a).starts_with("Assign statement")
                    || get_title(a) == "Assign to new local variable in try-with-resources")),
            "{actions:#?}"
        );
        if name == "no-resource-unreachable" {
            for kind in ["refactor.assign.variable", "refactor.assign.field"] {
                assert!(actions.iter().any(|a| a["kind"] == kind), "{actions:#?}");
            }
        } else {
            assert!(
                !actions.iter().any(|a| a["kind"]
                    .as_str()
                    .is_some_and(|k| k.starts_with("refactor.assign"))),
                "{actions:#?}"
            );
        }
        return;
    }
    let label = if name.starts_with("local-") {
        "Assign statement to new local variable"
    } else if name.starts_with("field-") {
        "Assign statement to new field"
    } else if name.starts_with("resource-") {
        "Assign to new local variable in try-with-resources"
    } else if name.starts_with("return-") {
        "Return the allocated object"
    } else if name.starts_with("throw-") {
        "Throw the allocated object"
    } else {
        "Remove"
    };
    let action = actions
        .iter()
        .find(|a| get_title(a) == label)
        .unwrap_or_else(|| panic!("{actions:#?}"));
    assert_eq!(action["kind"], "quickfix");
    let result = t.evaluate_code_action_command(action);
    let expected = match name {
        "local-scope-static" => include_str!("fixtures/allocation-corrections/local-scope-static.java"),
        "local-scope-instance" => include_str!("fixtures/allocation-corrections/local-scope-instance.java"),
        "local-scope-inherited" => include_str!("fixtures/allocation-corrections/local-scope-inherited.java"),
        "resource-lambda-declared" => "package p;\n\nimport java.io.ByteArrayInputStream;\n\npublic class A {\n    interface Job { void run() throws java.io.IOException; }\n    void run() {\n        Job job = () -> {\n            try (ByteArrayInputStream byteArrayInputStream = new java.io.ByteArrayInputStream(new byte[0])) {\n                \n            };\n        };\n    }\n}\n",
        "field-static" => include_str!("fixtures/allocation-corrections/field-static.java"),
        "field-this" => include_str!("fixtures/allocation-corrections/field-this.java"),
        "local-affix" => include_str!("fixtures/allocation-corrections/local-affix.java"),
        "local-anonymous" => include_str!("fixtures/allocation-corrections/local-anonymous.java"),
        "local-collision" => include_str!("fixtures/allocation-corrections/local-collision.java"),
        "local-control" => include_str!("fixtures/allocation-corrections/local-control.java"),
        "local-generic" => include_str!("fixtures/allocation-corrections/local-generic.java"),
        "local-keyword" => include_str!("fixtures/allocation-corrections/local-keyword.java"),
        "resource-declared" => "package p;\n\nimport java.io.ByteArrayInputStream;\n\npublic class A {\n    void run() throws java.io.IOException {\n        try (ByteArrayInputStream byteArrayInputStream = new java.io.ByteArrayInputStream(new byte[0])) {\n            \n        };\n    }\n}\n",
        "resource-empty" => "package p;\n\nimport java.io.ByteArrayInputStream;\nimport java.io.IOException;\n\npublic class A {\n    void run() {\n        try (ByteArrayInputStream byteArrayInputStream = new java.io.ByteArrayInputStream(new byte[0])) {\n            \n        } catch (IOException e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        };\n    }\n}\n",
        "resource-existing" => include_str!("fixtures/allocation-corrections/resource-existing.java"),
        "resource-rethrow" => "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    static class Resource implements AutoCloseable {\n        Resource() throws IOException {}\n        public void close() throws Exception {}\n    }\n    void run() throws IOException {\n        try (Resource resource = new Resource()) {\n            \n        } catch (IOException e) {\n            throw e;\n        } catch (Exception e) {\n            // TODO Auto-generated catch block\n            e.printStackTrace();\n        };\n    }\n}\n",
        "return-comments" => include_str!("fixtures/allocation-corrections/return-comments.java"),
        _ => panic!("unknown fixture {name}"),
    };
    assert_eq!(result, expected);
}

#[test]
fn return_is_absent_in_constructors_initializers_and_lambdas() {
    for source in [
        "package p; public class A { A() {new String();} }",
        "package p; public class A { {new String();} }",
        "package p; public class A { void run(){Runnable r = () -> {new String();};} }",
    ] {
        let mut t = QuickFixTest::new();
        let mut options = test_default_options();
        options.insert(
            "org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(),
            "error".into(),
        );
        let root = t.ws.new_empty_project(&options);
        let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
        let actions = t.evaluate_code_actions_range(&uri, get_range(source, "new String()"));
        assert!(
            actions
                .iter()
                .any(|a| get_title(a) == "Assign statement to new local variable"),
            "{actions:#?}"
        );
        assert!(
            !actions
                .iter()
                .any(|a| get_title(a) == "Return the allocated object"),
            "{actions:#?}"
        );
    }
}

#[test]
fn return_compatibility_controls_proposal_order() {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(),
        "error".into(),
    );
    let root = t.ws.new_empty_project(&options);
    let source = "package p; public class A { Object run(){new String();return null;} }";
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    let actions = t.evaluate_code_actions_range(&uri, get_range(source, "new String()"));
    let titles: Vec<_> = actions.iter().map(get_title).collect();
    let ret = titles
        .iter()
        .position(|s| s == "Return the allocated object")
        .unwrap();
    let local = titles
        .iter()
        .position(|s| s == "Assign statement to new local variable")
        .unwrap();
    assert!(ret < local, "{titles:#?}");
}

#[test]
fn local_preserves_generic_type() {
    run("local-generic", "package p;\n\npublic class A {\n    void run() {\n        new java.util.ArrayList<String>();\n    }\n}\n", "new java.util.ArrayList<String>()");
}
#[test]
fn local_control_body_becomes_block() {
    run("local-control", "package p;\n\npublic class A {\n    void run(boolean flag) {\n        if (flag)\n            new String();\n    }\n}\n", "new String()");
}
#[test]
fn local_avoids_names_before_and_after_selection() {
    run("local-collision", "package p;\n\npublic class A {\n    String string;\n    void run(String string2) {\n        new String();\n        String string3 = null;\n    }\n}\n", "new String()");
}
#[test]
fn local_obeys_affix_preferences() {
    run(
        "local-affix",
        "package p;\n\npublic class A {\n    void run() {\n        new String();\n    }\n}\n",
        "new String()",
    );
}
#[test]
fn field_is_static_in_static_method() {
    run("field-static", "package p;\n\npublic class A {\n    int count;\n    static void run() {\n        new String();\n    }\n    String later;\n}\n", "new String()");
}
#[test]
fn field_obeys_this_preference() {
    run(
        "field-this",
        "package p;\n\npublic class A {\n    void run() {\n        new String();\n    }\n}\n",
        "new String()",
    );
}
#[test]
fn resource_includes_close_exception_and_empty_body() {
    run("resource-empty", "package p;\n\npublic class A {\n    void run() {\n        new java.io.ByteArrayInputStream(new byte[0]);\n    }\n}\n", "new java.io.ByteArrayInputStream(new byte[0])");
}
#[test]
fn resource_rethrows_declared_exception() {
    run("resource-rethrow", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    static class Resource implements AutoCloseable {\n        Resource() throws IOException {}\n        public void close() throws Exception {}\n    }\n    void run() throws IOException {\n        new Resource();\n    }\n}\n", "new Resource()");
}
#[test]
fn unreachable_catch_keeps_assignments_as_refactors() {
    run("no-resource-unreachable", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void run() {\n        try {\n            new java.io.ByteArrayInputStream(new byte[0]);\n            System.out.println(1);\n        } catch (IOException ex) {\n            ex.printStackTrace();\n        }\n    }\n}\n", "new java.io.ByteArrayInputStream(new byte[0])");
}
#[test]
fn resource_extends_existing_try() {
    run("resource-existing", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        try {\n            new java.io.ByteArrayInputStream(new byte[0]);\n            read();\n        } catch (IOException ex) {\n            ex.printStackTrace();\n        }\n    }\n}\n", "new java.io.ByteArrayInputStream(new byte[0])");
}
#[test]
fn return_preserves_leading_and_trailing_comments() {
    run("return-comments", "package p;\n\npublic class A {\n    String run() {\n        // before\n        new String(); // after\n        return null;\n    }\n}\n", "new String()");
}

#[test]
fn local_normalizes_anonymous_class_type() {
    run("local-anonymous", "package p;\n\npublic class A {\n    void run() {\n        new Runnable() { public void run() {} };\n    }\n}\n", "new Runnable() { public void run() {} }");
}
#[test]
fn local_avoids_keyword() {
    run("local-keyword", "package p;\n\npublic class A {\n    static class Default {}\n    void run() {\n        new Default();\n    }\n}\n", "new Default()");
}
#[test]
fn missing_semicolon_suppresses_assignment_refactors() {
    run(
        "no-recovered",
        "package p;\n\npublic class A {\n    void run() {\n        new String()\n    }\n}\n",
        "new String()",
    );
}
#[test]
fn resource_uses_declared_close_contract() {
    run("resource-declared", "package p;\n\npublic class A {\n    void run() throws java.io.IOException {\n        new java.io.ByteArrayInputStream(new byte[0]);\n    }\n}\n", "new java.io.ByteArrayInputStream(new byte[0])");
}

#[test]
fn virtual_documents_offer_allocation_options() {
    if is_oracle() {
        return;
    } // Eclipse's virtual units require a resource on disk.
    for scheme in ["untitled", "inmemory", "file"] {
        for (source, selection, title, expected) in [
            ("class A {void run(){new String();}}", "new String()", "Assign statement to new local variable", "Stringstring=newString();"),
            ("class A {void run(){new String();}}", "new String()", "Assign statement to new field", "string=newString();"),
            ("class A {void run(){new RuntimeException();}}", "new RuntimeException()", "Throw the allocated object", "thrownewRuntimeException();"),
            ("class A {void run(){new java.io.ByteArrayInputStream(new byte[0]);}}", "new java.io.ByteArrayInputStream(new byte[0])", "Assign to new local variable in try-with-resources", "try(ByteArrayInputStreambyteArrayInputStream=newjava.io.ByteArrayInputStream(newbyte[0]))"),
        ] {
            let mut ws = Workspace::new();
            ws.capabilities = quickfix_client_capabilities();
            ws.init_options["compilerOptions"] = json!({"org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation":"error"});
            let path = ws.dir.join("A.java");
            let uri = match scheme {
                "untitled" => "untitled:A.java".to_owned(),
                "inmemory" => "inmemory:///A.java".to_owned(),
                _ => tower_lsp::lsp_types::Url::from_file_path(&path).unwrap().to_string(),
            };
            ws.open_with(&uri, source);
            let diagnostics = ws.diagnostics(&uri);
            let actions = ws.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":get_range(source,selection),"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
            let action = actions.as_array().unwrap().iter().find(|a| a["title"] == title).unwrap_or_else(|| panic!("{scheme}: {actions:#?}"));
            let edit = &action["edit"];
            let edits = edit["changes"][&uri].as_array().or_else(|| edit["documentChanges"].as_array().and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri)).and_then(|c| c["edits"].as_array())).unwrap();
            let result = apply_edits(source, edits);
            let compact: String = result.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(compact.contains(expected), "{scheme}: {result}");
            if title == "Assign statement to new field" { assert!(compact.contains("privateStringstring;"), "{result}"); }
            if title.contains("try-with-resources") { assert!(result.contains("import java.io.IOException;"), "{result}"); }
            assert!(!path.exists());
        }
    }
}

#[test]
fn ordinary_expression_assignments_use_refactor_kinds_and_final_preferences() {
    for (field, final_setting) in [(false, "variables"), (true, "fields")] {
        let mut t = QuickFixTest::new();
        t.ws.settings["java"]["codeGeneration"]["addFinalForNewDeclaration"] = json!(final_setting);
        let root = t.ws.new_empty_project(&test_default_options());
        let source = "package p;\n\npublic class A {\n    String getLabel(){return null;}\n    void run(){\n        getLabel();\n    }\n}\n";
        let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
        let actions = t.evaluate_code_actions_range(&uri, get_range(source, "getLabel()"));
        let kind = if field {
            "refactor.assign.field"
        } else {
            "refactor.assign.variable"
        };
        let action = actions
            .iter()
            .find(|a| a["kind"] == kind)
            .unwrap_or_else(|| panic!("{actions:#?}"));
        let result = t.evaluate_code_action_command(action);
        let expected = if field {
            include_str!("fixtures/allocation-corrections/refactor-fields.java")
        } else {
            include_str!("fixtures/allocation-corrections/refactor-variables.java")
        };
        assert_eq!(result, expected);
    }
}

#[test]
fn advanced_assignments_return_edits_and_exact_utf16_rename_positions() {
    for (name, field, source, selection, expected) in [
        ("generic", false, "package p;\n\npublic class A {\n    void run() {\n        new java.util.ArrayList<String>();\n    }\n}\n", "new java.util.ArrayList<String>()", include_str!("fixtures/allocation-corrections/local-generic.java")),
        ("control", false, "package p;\n\npublic class A {\n    void run(boolean flag) {\n        if (flag)\n            new String();\n    }\n}\n", "new String()", include_str!("fixtures/allocation-corrections/local-control.java")),
        ("field-this", true, "package p;\n\npublic class A {\n    void run() {\n        new String();\n    }\n}\n", "new String()", include_str!("fixtures/allocation-corrections/field-this.java")),
        ("field-static", true, "package p;\n\npublic class A {\n    int count;\n    static void run() {\n        new String();\n    }\n    String later;\n}\n", "new String()", include_str!("fixtures/allocation-corrections/field-static.java")),
        ("unicode", false, "package p;\n\npublic class A {\n    void run() {\n        // 🧪 before allocation\n        new String();\n    }\n}\n", "new String()", "package p;\n\npublic class A {\n    void run() {\n        // 🧪 before allocation\n        String string = new String();\n    }\n}\n"),
    ] {
        let mut t = QuickFixTest::new();
        t.ws.init_options["extendedClientCapabilities"]["advancedExtractRefactoringSupport"] = json!(true);
        let mut options = test_default_options();
        options.insert("org.eclipse.jdt.core.compiler.problem.unusedObjectAllocation".into(), "ignore".into());
        let root = t.ws.new_empty_project(&options);
        if name == "field-this" {
            std::fs::write(root.join(".settings/org.eclipse.jdt.ls.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.ui.keywordthis=true\n").unwrap();
        }
        let uri = t.ws.create_cu(&root,"src","p","A.java",source);
        let actions = t.evaluate_code_actions_range(&uri, get_range(source, selection));
        let kind = if field { "refactor.assign.field" } else { "refactor.assign.variable" };
        let action = actions.iter().find(|a| a["kind"] == kind).unwrap_or_else(|| panic!("{name}: {actions:#?}"));
        assert!(action["edit"].is_null());
        let arguments = &action["command"]["arguments"];
        let response = t.ws.request("java/getRefactorEdit", json!({"command":arguments[0],"context":arguments[1]}));
        let edit = &response["edit"];
        let edits = edit["changes"][&uri].as_array().or_else(|| edit["documentChanges"].as_array().and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri)).and_then(|c| c["edits"].as_array())).unwrap_or_else(|| panic!("{name}: {response:#?}"));
        let result = apply_edits(source, edits);
        assert_eq!(result, expected, "{name}: {response:#?}");
        let variable = if name == "generic" {"arrayList"} else {"string"};
        let position = expected.find(&format!("{variable} =")).unwrap();
        let offset = expected[..position].encode_utf16().count();
        assert_eq!(response["command"], json!({"title":"Rename","command":"java.action.rename","arguments":[{"uri":uri,"offset":offset,"length":variable.encode_utf16().count()}]}), "{name}: {response:#?}");
    }
}

#[test]
fn resource_assignment_respects_lambda_declared_exceptions() {
    run("resource-lambda-declared", "package p;\n\npublic class A {\n    interface Job { void run() throws java.io.IOException; }\n    void run() {\n        Job job = () -> {\n            new java.io.ByteArrayInputStream(new byte[0]);\n        };\n    }\n}\n", "new java.io.ByteArrayInputStream(new byte[0])");
}

#[test]
fn assignments_and_void_calls_have_no_assignment_refactors() {
    for (source, selection) in [
        (
            "package p; public class A { int value; void run(){value = 1;} }",
            "value = 1",
        ),
        (
            "package p; public class A { void run(){System.out.println(1);} }",
            "System.out.println(1)",
        ),
    ] {
        let mut t = QuickFixTest::new();
        let root = t.ws.new_empty_project(&test_default_options());
        let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
        let actions = t.evaluate_code_actions_range(&uri, get_range(source, selection));
        assert!(
            !actions.iter().any(|a| a["kind"]
                .as_str()
                .is_some_and(|k| k.starts_with("refactor.assign"))),
            "{actions:#?}"
        );
    }
}

#[test]
fn virtual_documents_support_advanced_assignment_edits() {
    if is_oracle() {
        return;
    }
    for scheme in ["untitled", "inmemory", "file"] {
        for field in [false, true] {
            let mut ws = Workspace::new();
            ws.capabilities = quickfix_client_capabilities();
            ws.init_options["extendedClientCapabilities"] =
                json!({"advancedExtractRefactoringSupport":true});
            let source = "class A {void run(){String.valueOf(1);}}";
            let path = ws.dir.join("A.java");
            let uri = match scheme {
                "untitled" => "untitled:A.java".to_owned(),
                "inmemory" => "inmemory:///A.java".to_owned(),
                _ => tower_lsp::lsp_types::Url::from_file_path(&path)
                    .unwrap()
                    .to_string(),
            };
            ws.open_with(&uri, source);
            let context = json!({"textDocument":{"uri":uri},"range":get_range(source,"String.valueOf(1)"),"context":{"diagnostics":[]}});
            let response = ws.request("java/getRefactorEdit", json!({"command":if field {"assignField"} else {"assignVariable"},"context":context}));
            let edit = &response["edit"];
            let edits = edit["changes"][&uri]
                .as_array()
                .or_else(|| {
                    edit["documentChanges"]
                        .as_array()
                        .and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri))
                        .and_then(|c| c["edits"].as_array())
                })
                .unwrap_or_else(|| panic!("{scheme}: {response:#?}"));
            let result = apply_edits(source, edits);
            let compact: String = result.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(
                compact.contains(if field {
                    "valueOf=String.valueOf(1);"
                } else {
                    "StringvalueOf=String.valueOf(1);"
                }),
                "{scheme}: {result}"
            );
            if field {
                assert!(
                    compact.contains("privateStringvalueOf;"),
                    "{scheme}: {result}"
                );
            }
            let position = &response["command"]["arguments"][0];
            assert_eq!(position["uri"], uri);
            let offset = position["offset"].as_u64().unwrap() as usize;
            let length = position["length"].as_u64().unwrap() as usize;
            let units: Vec<_> = result.encode_utf16().collect();
            assert_eq!(
                String::from_utf16_lossy(&units[offset..offset + length]),
                "valueOf"
            );
            assert!(
                String::from_utf16_lossy(&units[offset + length..])
                    .trim_start()
                    .starts_with("="),
                "{result}"
            );
            assert!(!path.exists());
        }
    }
}

#[test]
fn nested_type_field_visibility_controls_variable_names() {
    for (name, source) in [
        ("local-scope-static", "package p;\n\npublic class A {\n    String string;\n    static class Inner {\n        void run(){new String();}\n    }\n}\n"),
        ("local-scope-instance", "package p;\n\npublic class A {\n    String string;\n    class Inner {\n        void run(){new String();}\n    }\n}\n"),
        ("local-scope-inherited", "package p;\n\npublic class A {\n    static class Base { String string; }\n    static class Inner extends Base {\n        void run(){new String();}\n    }\n}\n"),
    ] {
        run(name, source, "new String()");
    }
}
