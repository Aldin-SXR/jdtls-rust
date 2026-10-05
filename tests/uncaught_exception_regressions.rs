//! Exception corrections across lambda and method-reference boundaries.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{get_range, get_title, quickfix_client_capabilities, QuickFixTest};
use serde_json::json;

fn surround(name: &str, source: &str, selection: &str) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    let root = t.ws.new_empty_project(&options);
    if name.starts_with("custom-") {
        let xml = "<templates><template id=\"org.eclipse.jdt.ui.text.codetemplates.catchblock\" name=\"catchblock\" description=\"catchblock\" context=\"catchblock_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">// ${exception_type} ${exception_var} in ${enclosing_type}.${enclosing_method}\n${exception_var}.printStackTrace();</template></templates>";
        std::fs::write(root.join(".settings/org.eclipse.jdt.ls.core.prefs"), format!("eclipse.preferences.version=1\norg.eclipse.jdt.ui.exception.name=failure\norg.eclipse.jdt.ui.text.custom_code_templates={}\n",xml.replace('\n',"\\n"))).unwrap();
    }
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    let actions = t.evaluate_code_actions_range(&uri, get_range(source, selection));
    if name.contains("lambda") || name.contains("reference") || name == "parameter-collision" {
        assert!(
            !actions
                .iter()
                .any(|a| get_title(a) == "Add throws declaration"),
            "lambda exceptions must be handled inside the lambda: {actions:#?}"
        );
    }
    let action = actions
        .iter()
        .find(|a| {
            get_title(a)
                == if name.starts_with("resource-") || name == "custom-resource" {
                    "Surround with try-with-resources"
                } else if name == "escaping-var" {
                    "Change type of 'value' to 'String' and surround with try/catch"
                } else {
                    "Surround with try/catch"
                }
        })
        .unwrap_or_else(|| panic!("{actions:#?}"));
    let result = t.evaluate_code_action_command(action);
    let expected = match name {
        "void-lambda" => include_str!("fixtures/uncaught-exceptions/void-lambda.java"),
        "value-lambda" => include_str!("fixtures/uncaught-exceptions/value-lambda.java"),
        "block-lambda" => include_str!("fixtures/uncaught-exceptions/block-lambda.java"),
        "bound-reference" => include_str!("fixtures/uncaught-exceptions/bound-reference.java"),
        "static-reference" => include_str!("fixtures/uncaught-exceptions/static-reference.java"),
        "creation-reference" => {
            include_str!("fixtures/uncaught-exceptions/creation-reference.java")
        }
        "unbound-reference" => include_str!("fixtures/uncaught-exceptions/unbound-reference.java"),
        "parameter-collision" => {
            include_str!("fixtures/uncaught-exceptions/parameter-collision.java")
        }
        "escaping-final" => include_str!("fixtures/uncaught-exceptions/escaping-final.java"),
        "escaping-var" => include_str!("fixtures/uncaught-exceptions/escaping-var.java"),
        "resource-lifetime" => include_str!("fixtures/uncaught-exceptions/resource-lifetime.java"),
        "resource-rethrow" => include_str!("fixtures/uncaught-exceptions/resource-rethrow.java"),
        "resource-existing-try" => {
            include_str!("fixtures/uncaught-exceptions/resource-existing-try.java")
        }
        "comment-outside" => include_str!("fixtures/uncaught-exceptions/comment-outside.java"),
        "comment-selected" => include_str!("fixtures/uncaught-exceptions/comment-selected.java"),
        "custom-single" => include_str!("fixtures/uncaught-exceptions/custom-single.java"),
        "custom-resource" => include_str!("fixtures/uncaught-exceptions/custom-resource.java"),
        _ => panic!("unknown fixture {name}"),
    };
    assert_eq!(result, expected);
}

#[test]
fn void_expression_lambda() {
    surround("void-lambda", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        Runnable r = () -> read();\n    }\n}\n", "read()");
}
#[test]
fn value_expression_lambda() {
    surround("value-lambda", "package p;\n\nimport java.io.IOException;\nimport java.util.function.Supplier;\n\npublic class A {\n    String read() throws IOException { return null; }\n    void run() {\n        Supplier<String> r = () -> read();\n    }\n}\n", "read()");
}
#[test]
fn block_lambda() {
    surround("block-lambda", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        Runnable r = () -> { read(); };\n    }\n}\n", "read();");
}
#[test]
fn bound_method_reference() {
    surround("bound-reference", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        Runnable r = this::read;\n    }\n}\n", "this::read");
}
#[test]
fn static_method_reference() {
    surround("static-reference", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    static void read() throws IOException {}\n    void run() {\n        Runnable r = A::read;\n    }\n}\n", "A::read");
}
#[test]
fn creation_reference() {
    surround("creation-reference", "package p;\n\nimport java.io.IOException;\nimport java.util.function.Supplier;\n\npublic class A {\n    A() throws IOException {}\n    void run() {\n        Supplier<A> r = A::new;\n    }\n}\n", "A::new");
}

#[test]
fn unbound_reference_parameter() {
    surround("unbound-reference", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    interface Action { void apply(A receiver); }\n    void read() throws IOException {}\n    void run() {\n        Action r = A::read;\n    }\n}\n", "A::read");
}
#[test]
fn reference_parameter_collision() {
    surround("parameter-collision", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    interface Action { void apply(String text); }\n    void read(String value) throws IOException {}\n    void run(String text) {\n        Action r = this::read;\n    }\n}\n", "this::read");
}
#[test]
fn escaping_final_local() {
    surround("escaping-final", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    String read() throws IOException { return null; }\n    void run() {\n        final String value = read();\n        System.out.println(value);\n    }\n}\n", "final String value = read();");
}
#[test]
fn escaping_var_local() {
    surround("escaping-var", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    String read() throws IOException { return null; }\n    void run() {\n        var value = read();\n        System.out.println(value);\n    }\n}\n", "var value = read();");
}

#[test]
fn resource_lifetime_moves_dependent_locals() {
    surround("resource-lifetime", "package p;\n\nimport java.io.ByteArrayInputStream;\n\npublic class A {\n    void run() {\n        ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);\n        int value = in.read();\n        int twice = value * 2;\n        System.out.println(twice);\n    }\n}\n", "ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);");
}
#[test]
fn resource_rethrows_declared_narrower_exception() {
    surround("resource-rethrow", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    static class Resource implements AutoCloseable {\n        public void close() throws Exception {}\n        void read() throws IOException {}\n    }\n    void run() throws IOException {\n        Resource in = new Resource();\n        in.read();\n    }\n}\n", "Resource in = new Resource();");
}
#[test]
fn resource_extends_existing_try() {
    surround("resource-existing-try", "package p;\n\nimport java.io.ByteArrayInputStream;\nimport java.io.IOException;\n\npublic class A {\n    void run() {\n        try {\n            ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);\n            System.out.println(in.read());\n        } catch (IOException ex) {\n            ex.printStackTrace();\n        }\n    }\n}\n", "ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);");
}

#[test]
fn leading_comment_outside_selection_stays_before_try() {
    surround("comment-outside", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        // keep here\n        read();\n    }\n}\n", "read();");
}
#[test]
fn selected_comments_keep_leading_and_move_trailing() {
    surround("comment-selected", "package p;\n\nimport java.io.IOException;\n\npublic class A {\n    void read() throws IOException {}\n    void run() {\n        // move inside\n        read(); // trailing\n    }\n}\n", "// move inside\n        read(); // trailing");
}
#[test]
fn custom_single_catch_uses_imported_type_name() {
    surround("custom-single", "package p;\n\npublic class A {\n    static class IOException {}\n    void read() throws java.io.IOException {}\n    void run() {\n        read();\n    }\n}\n", "read();");
}
#[test]
fn custom_resource_catch_uses_general_exception_type() {
    surround("custom-resource", "package p;\n\nimport java.io.ByteArrayInputStream;\n\npublic class A {\n    void run() {\n        ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);\n        System.out.println(in.read());\n    }\n}\n", "ByteArrayInputStream in = new ByteArrayInputStream(new byte[0]);");
}

#[test]
fn virtual_documents_offer_uncaught_and_resource_corrections() {
    if is_oracle() {
        return;
    } // JDT LS requires a resource-backed compilation unit.
    for scheme in ["untitled", "inmemory", "file"] {
        for (source, selection, title, expected) in [
            ("class A {void read() throws java.io.IOException {} void run(){read();}}", "read();", "Surround with try/catch", "try{read();}catch(IOExceptione)"),
            ("class A {void run(){java.io.ByteArrayInputStream in = new java.io.ByteArrayInputStream(new byte[0]);System.out.println(in.read());}}", "java.io.ByteArrayInputStream in = new java.io.ByteArrayInputStream(new byte[0]);", "Surround with try-with-resources", "try(java.io.ByteArrayInputStreamin=newjava.io.ByteArrayInputStream(newbyte[0])){System.out.println(in.read());}catch(IOExceptione)"),
        ] {
            let mut ws = Workspace::new();
            ws.capabilities = quickfix_client_capabilities();
            let path = ws.dir.join("A.java");
            let uri = match scheme {
                "untitled" => "untitled:A.java".to_owned(),
                "inmemory" => "inmemory:///A.java".to_owned(),
                _ => tower_lsp::lsp_types::Url::from_file_path(&path).unwrap().to_string(),
            };
            ws.open_with(&uri, source);
            let diagnostics = ws.diagnostics(&uri);
            let actions = ws.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":get_range(source, selection),"context":{"diagnostics":diagnostics}}));
            let action = actions.as_array().unwrap().iter().find(|a| a["title"] == title).unwrap_or_else(|| panic!("{scheme}: {actions:#?}"));
            let edit = &action["edit"];
            let edits = edit["changes"][&uri].as_array().or_else(|| edit["documentChanges"].as_array().and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri)).and_then(|c| c["edits"].as_array())).unwrap();
            let result = apply_edits(source, edits);
            let compact: String = result.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(compact.contains(expected), "{scheme}: {result}");
            assert!(result.contains("import java.io.IOException;"), "{result}");
            assert!(!path.exists());
        }
    }
}
