//! Abstract/native/body and inherited implementation corrections, oracle checked.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{quickfix_client_capabilities, QuickFixTest};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
const ADD: &str = "Add unimplemented methods";
fn setup(source: &str) -> (QuickFixTest, PathBuf, String) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    let root = t.ws.new_empty_project(&options);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    (t, root, uri)
}
fn action(t: &mut QuickFixTest, uri: &str, title: &str) -> Value {
    let actions = t.evaluate_code_actions(uri);
    actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("Missing {title}: {actions:#?}; diagnostics={:#?}", t.diagnostics(uri)))
        .clone()
}
fn fixed(t: &mut QuickFixTest, uri: &str, title: &str) -> String {
    let a = action(t, uri, title);
    assert_eq!(a["kind"], "quickfix");
    t.evaluate_code_action_command(&a)
}
fn run(source: &str, title: &str) -> String {
    let (mut t, _, uri) = setup(source);
    fixed(&mut t, &uri, title)
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, fragments: &[&str]) {
    for fragment in fragments {
        assert!(
            compact(s).contains(&compact(fragment)),
            "Missing {fragment:?}: {s}"
        );
    }
}
fn prefs(root: &Path, entries: &[(&str, &str)]) {
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        entries
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>(),
    )
    .unwrap();
}
fn template(key: &str, body: &str) -> String {
    let body = body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "&#10;");
    format!("<templates><template id=\"org.eclipse.jdt.ui.text.codetemplates.{key}\" name=\"{key}\" description=\"{key}\" context=\"{key}_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{body}</template></templates>")
}
#[test]
fn abstract_body_removal_keeps_annotations_and_allowed_modifiers() {
    let s = run("package p; public abstract class A { @Deprecated public final synchronized abstract int number() {return 7;} }", "Remove method body");
    contains(&s, &["@Deprecated public abstract int number();"]);
    assert!(!s.contains("synchronized") && !s.contains("final"), "{s}");
}
#[test]
fn removing_abstract_preserves_existing_body() {
    let s = run(
        "package p; public abstract class A { public abstract int number() {return 7;} }",
        "Remove 'abstract' modifier",
    );
    contains(&s, &["public int number() {return 7;}"]);
}
#[test]
fn primitive_and_reference_missing_bodies() {
    for (typ, expression) in [
        ("boolean", "false"),
        ("int", "0"),
        ("double", "0"),
        ("String", "null"),
        ("int[]", "null"),
    ] {
        let s = run(
            &format!("package p; public class A {{ public {typ} number(); }}"),
            "Add body",
        );
        contains(
            &s,
            &[&format!("public {typ} number() {{ return {expression}; }}")],
        );
    }
}
#[test]
fn old_style_array_return_uses_null() {
    let s = run(
        "package p; public class A { public int number()[]; }",
        "Add body",
    );
    contains(&s, &["int number()[] { return null; }"]);
}
#[test]
fn fully_qualified_optional_and_imported_optional_differ() {
    for (prefix, typ, expression) in [
        (
            "",
            "java.util.Optional<String>",
            "java.util.Optional.empty()",
        ),
        ("import java.util.Optional;", "Optional<String>", "null"),
    ] {
        let s = run(
            &format!("package p; {prefix} public class A {{ public {typ} number(); }}"),
            "Add body",
        );
        contains(&s, &[&format!("return {expression};")]);
    }
}
#[test]
fn native_body_replaced_or_removed() {
    let (mut t, _, uri) =
        setup("package p; public class A { public native boolean number() {return true;} }");
    contains(
        &fixed(&mut t, &uri, "Remove 'native' modifier"),
        &["public boolean number() {return false;}"],
    );
    contains(
        &fixed(&mut t, &uri, "Remove method body"),
        &["public native boolean number();"],
    );
}
#[test]
fn constructor_missing_body_is_empty() {
    contains(
        &run("package p; public class A { public A(); }", "Add body"),
        &["public A() {}"],
    );
}
#[test]
fn abstract_type_modifier_is_appended_even_after_final() {
    let s = run(
        "package p; public final class A { public abstract void run(); }",
        "Make type 'A' abstract",
    );
    contains(&s, &["public final abstract class A"]);
}
#[test]
fn missing_static_body_can_be_made_abstract() {
    let s = run(
        "package p; public abstract class A { @Deprecated public static int number(); }",
        "Change 'A.number' to 'abstract'",
    );
    contains(&s, &["@Deprecated public abstract int number();"]);
}
#[test]
fn interface_abstract_body_can_be_static_or_default() {
    let (mut t, _, uri) =
        setup("package p; public interface A { public abstract int number() {return 7;} }");
    contains(
        &fixed(&mut t, &uri, "Change 'number' to 'static'"),
        &["public static int number() {return 7;}"],
    );
    contains(
        &fixed(&mut t, &uri, "Change 'number' to 'default'"),
        &["public default int number() {return 7;}"],
    );
}
#[test]
fn inherited_parameterized_generic_varargs_and_throws() {
    let s = run("package p; public class A implements I<String> {} interface I<T> { T echo(T text) throws java.io.IOException; <N extends Number & Comparable<N>> N number(N value, String... labels); }", ADD);
    contains(
        &s,
        &[
            "import java.io.IOException;",
            "public String echo(String text) throws IOException",
            "public <N extends Number & Comparable<N>> N number(N value, String... labels)",
            "throw new UnsupportedOperationException(\"Unimplemented method 'echo'\");",
        ],
    );
    assert!(!s.contains("return null;"), "{s}");
}
#[test]
fn covariant_interfaces_choose_most_specific_return() {
    let s = run("package p; public class A implements I, J {} interface I { Object value(); } interface J { String value(); }", ADD);
    contains(&s, &["public String value()"]);
    assert_eq!(s.matches("public String value()").count(), 1, "{s}");
    assert!(!s.contains("public Object value()"), "{s}");
}
#[test]
fn inherited_concrete_and_default_methods_are_suppressed() {
    let s = run("package p; public class A extends B implements I {} class B { public void done() {} } interface I { void done(); default void optional() {} void missing(); }", ADD);
    contains(&s, &["public void missing()"]);
    assert_eq!(s.matches("public void done()").count(), 1, "{s}");
    assert!(!s.contains("public void optional()"), "{s}");
}
#[test]
fn abstract_redeclaration_of_default_is_implemented() {
    let s = run("package p; public class A implements J {} interface I { default void run() {} } interface J extends I { void run(); }", ADD);
    contains(&s, &["public void run()", "Unimplemented method 'run'"]);
}
#[test]
fn source_order_and_reverse_interface_order() {
    let s = run("package p; public class A extends B implements I, J {} abstract class B { protected abstract void superOne(); } interface I { void zebra(); void alpha(); } interface J { void lastInterface(); }", ADD);
    let positions: Vec<_> = [
        "public void lastInterface()",
        "public void zebra()",
        "public void alpha()",
        "protected void superOne()",
    ]
    .into_iter()
    .map(|n| s.find(n).unwrap_or_else(|| panic!("{s}")))
    .collect();
    assert!(positions.windows(2).all(|p| p[0] < p[1]), "{s}");
}
#[test]
fn enum_constant_gets_anonymous_body() {
    let s = run(
        "package p; public enum A { ONE; public abstract int number(); }",
        ADD,
    );
    contains(
        &s,
        &[
            "ONE { @Override public int number()",
            "Unimplemented method 'number'",
            "public abstract int number();",
        ],
    );
}
#[test]
fn existing_enum_constant_body_is_preserved() {
    let s = run(
        "package p; public enum A { ONE { void own() {} }; public abstract int number(); }",
        ADD,
    );
    contains(&s, &["ONE { void own() {}", "public int number()"]);
    assert_eq!(s.matches("void own()").count(), 1);
}
#[test]
fn anonymous_implementation_is_inside_the_creation() {
    let s = run(
        "package p; public class A { I value = new I() {}; } interface I { int number(); }",
        ADD,
    );
    contains(&s, &["new I() { @Override public int number()"]);
    assert!(
        s.find("public int number()").unwrap() < s.find("interface I").unwrap(),
        "{s}"
    );
}
#[test]
fn ordinary_body_template_uses_declaring_type_and_default_statement() {
    let (mut t, root, uri) =
        setup("package p; public class A implements I {} interface I { int number(); }");
    let xml = template(
        "methodbody",
        "// ${enclosing_type}.${enclosing_method}\n${body_statement}",
    );
    prefs(
        &root,
        &[("org.eclipse.jdt.ui.text.custom_code_templates", &xml)],
    );
    contains(&fixed(&mut t, &uri, ADD), &["// I.number", "return 0;"]);
}
#[test]
fn override_annotation_project_preference_is_respected() {
    let (mut t, root, uri) =
        setup("package p; public class A implements I {} interface I { int number(); }");
    prefs(&root, &[("org.eclipse.jdt.ui.overrideannotation", "false")]);
    let s = fixed(&mut t, &uri, ADD);
    contains(&s, &["public int number()"]);
    assert!(!s.contains("@Override"), "{s}");
}
#[test]
fn custom_inherited_comment_uses_project_preference() {
    let (mut t, root, uri) = setup("package p; public class A implements I {} interface I { String echo(String text) throws java.io.IOException; }");
    let xml = template(
        "overridecomment",
        "/**\n * ${enclosing_type}.${enclosing_method}\n * ${tags}\n * ${see_to_overridden}\n */",
    );
    prefs(
        &root,
        &[
            ("org.eclipse.jdt.ui.javadoc", "true"),
            ("org.eclipse.jdt.ui.text.custom_code_templates", &xml),
        ],
    );
    contains(
        &fixed(&mut t, &uri, ADD),
        &[
            "* I.echo",
            "* @param text",
            "* @return",
            "* @throws IOException",
            "* @see p.I#echo(java.lang.String)",
        ],
    );
}
#[test]
fn anonymous_classes_suppress_project_comments() {
    let (mut t, root, uri) =
        setup("package p; public class A { I value = new I() {}; } interface I { int number(); }");
    let xml = template("overridecomment", "/** CUSTOM ${enclosing_method} */");
    prefs(
        &root,
        &[
            ("org.eclipse.jdt.ui.javadoc", "true"),
            ("org.eclipse.jdt.ui.text.custom_code_templates", &xml),
        ],
    );
    assert!(!fixed(&mut t, &uri, ADD).contains("CUSTOM"));
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
fn deferred_quick_fix_resolves_with_diagnostics_and_resource_edits() {
    let (mut t, _, uri) =
        setup("package p; public class A implements I {} interface I { int number(); }");
    t.ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    t.ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        json!(["create", "rename", "delete"]);
    let a = action(&mut t, &uri, ADD);
    assert!(a["edit"].is_null(), "{a}");
    assert!(!a["diagnostics"].as_array().unwrap().is_empty(), "{a}");
    let resolved = t.ws.request("codeAction/resolve", a);
    assert!(resolved["edit"]["documentChanges"].is_array(), "{resolved}");
    let s = apply_edits(&t.ws.read(&uri), edits(&resolved["edit"], &uri));
    contains(&s, &["public int number()"]);
}
#[test]
fn open_buffer_unicode_crlf_preserves_disk() {
    let disk = "package p; public class A {}";
    let source = "package p;\r\n// 😀 café\r\npublic class A implements I {}\r\ninterface I { int number(); }\r\n";
    let (mut t, _, uri) = setup(disk);
    // Register the document in QuickFixTest before replacing its open buffer.
    t.diagnostics(&uri);
    t.ws.change(&uri, source);
    let a = action(&mut t, &uri, ADD);
    let s = apply_edits(source, edits(&a["edit"], &uri));
    contains(&s, &["public int number()", "// 😀 café"]);
    assert!(!s.replace("\r\n", "").contains('\n'), "{s:?}");
    assert_eq!(t.ws.read(&uri), disk);
}
#[test]
fn virtual_documents_support_real_diagnostics_and_quick_fixes() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Methods.java",
        "inmemory://parity/Methods.java",
        "file:///tmp/jdtls-parity-missing-methods/A.java",
    ] {
        let source = "public class A implements I {} interface I { int number(); }";
        let mut ws = Workspace::new();
        ws.capabilities = quickfix_client_capabilities();
        ws.open_with(uri, source);
        let diagnostics = ws.diagnostics(uri);
        let diagnostic = diagnostics
            .iter()
            .find(|d| d["message"].as_str().unwrap_or("").contains("implement"))
            .unwrap_or_else(|| panic!("{diagnostics:#?}"));
        let actions = ws.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":diagnostic["range"],"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
        let a = actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == ADD)
            .unwrap_or_else(|| panic!("{actions:#?}"));
        contains(
            &apply_edits(source, edits(&a["edit"], uri)),
            &["public int number()"],
        );
    }
}
#[test]
fn own_abstract_method_is_retained_and_implemented_when_another_is_missing() {
    let s = run("package p; public class A implements I { public abstract void own(); } interface I { void missing(); }", ADD);
    contains(
        &s,
        &[
            "public abstract void own();",
            "public void own()",
            "public void missing()",
        ],
    );
    assert!(
        s.find("public void missing()").unwrap() < s.find("public void own()").unwrap(),
        "{s}"
    );
}
#[test]
fn protected_abstract_superclass_method_from_another_package() {
    let (mut t, root, uri) = setup("package p; public class A extends q.B {}");
    t.ws.create_cu(&root, "src", "q", "B.java", "package q; public abstract class B { protected abstract java.util.List<String> values(); }");
    contains(
        &fixed(&mut t, &uri, ADD),
        &["import java.util.List;", "protected List<String> values()"],
    );
}
#[test]
fn generated_types_avoid_existing_simple_name_conflicts() {
    let s = run("package p; public class A implements I { List existing; } class List {} interface I { java.util.List<String> values(); }", ADD);
    contains(&s, &["public java.util.List<String> values()"]);
    assert!(!s.contains("import java.util.List;"), "{s}");
}
#[test]
fn compiler_interface_annotation_preference_is_respected() {
    let (mut t, root, uri) =
        setup("package p; public class A implements I {} interface I { int number(); }");
    let path = root.join(".settings/org.eclipse.jdt.core.prefs");
    let mut settings = std::fs::read_to_string(&path).unwrap();
    settings.push_str("\norg.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation=disabled\n");
    std::fs::write(path, settings).unwrap();
    let s = fixed(&mut t, &uri, ADD);
    contains(&s, &["public int number()"]);
    assert!(!s.contains("@Override"), "{s}");
}
