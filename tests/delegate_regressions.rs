//! Delegate discovery, source-action and generation cases beyond the eight upstream ports.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::PathBuf;
fn setup(source: &str, settings: Value) -> (Workspace, PathBuf, String) {
    let mut ws = Workspace::new();
    ws.settings = settings;
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(true);
    let mut options = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    let root = ws.new_empty_project(&options);
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    (ws, root, uri)
}
fn params(uri: &str, source: &str) -> Value {
    let token = if source.contains("public class A") {
        "public class A"
    } else if source.contains("public enum A") {
        "public enum A"
    } else if source.contains("public record A") {
        "public record A"
    } else if source.contains("public interface A") {
        "public interface A"
    } else {
        "class A"
    };
    json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})
}
fn open(ws: &mut Workspace, uri: &str) {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
}
fn status(ws: &mut Workspace, uri: &str, source: &str) -> Value {
    ws.request("java/checkDelegateMethodsStatus", params(uri, source))
}
fn fields(s: &Value) -> Vec<String> {
    s["delegateFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field"]["name"].as_str().unwrap().into())
        .collect()
}
fn methods(s: &Value, field: &str) -> Vec<String> {
    s["delegateFields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"]["name"] == field)
        .map(|f| {
            f["delegateMethods"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["name"].as_str().unwrap().into())
                .collect()
        })
        .unwrap_or_default()
}
fn select(s: &Value, field: &str, names: &[&str]) -> Vec<Value> {
    let f = s["delegateFields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"]["name"] == field)
        .expect("delegate field");
    f["delegateMethods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| names.contains(&m["name"].as_str().unwrap()))
        .map(|m| json!({"field":f["field"],"delegateMethod":m}))
        .collect()
}
fn generate(ws: &mut Workspace, uri: &str, source: &str, entries: Vec<Value>) -> String {
    let edit = ws.request(
        "java/generateDelegateMethods",
        json!({"context":params(uri,source),"delegateEntries":entries}),
    );
    assert!(edit["documentChanges"].is_null(), "{edit}");
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .unwrap_or_else(|| panic!("delegate edits: {edit}")),
    )
}
fn run(source: &str, names: &[&str]) -> String {
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    generate(&mut ws, &uri, source, select(&s, "b", names))
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, expected: &[&str]) {
    for e in expected {
        assert!(compact(s).contains(&compact(e)), "missing {e:?} in {s}");
    }
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, only: Option<Vec<&str>>) -> Value {
    let mut p = params(uri, source);
    if let Some(only) = only {
        p["context"]["only"] = json!(only);
    }
    ws.request("textDocument/codeAction", p)
}
fn prompt(a: &Value) -> Option<&Value> {
    a.as_array().unwrap().iter().find(|a| {
        a["command"]["command"] == "java.action.generateDelegateMethodsPrompt"
            || a["command"] == "java.action.generateDelegateMethodsPrompt"
    })
}
#[test]
fn primitive_array_enum_constant_and_static_field_discovery() {
    let source="package p; public enum A { ONE; int n; B[] array; static B first; B second; } class B { public void ping() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(fields(&s), vec!["first", "second"]);
    assert_eq!(methods(&s, "first"), vec!["ping", "toString"]);
}
#[test]
fn dto_keys_and_overload_parameter_names() {
    let source="package p; public class A { B b; } class B { public void send(String text, int[] counts) {} public void send(int count) {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        s["delegateFields"][0]["field"],
        json!({"bindingKey":"Lp/A;.b)Lp/A~B;","name":"b","type":"B","isField":true,"isSelected":false})
    );
    let ms = &s["delegateFields"][0]["delegateMethods"];
    assert_eq!(
        ms[0],
        json!({"bindingKey":"Lp/A~B;.send(Ljava/lang/String;[I)V","name":"send","parameters":["String","int[]"]})
    );
    assert_eq!(ms[1]["parameters"], json!(["int"]));
}
#[test]
fn nonpublic_static_constructor_and_final_object_exclusion() {
    let source="package p; public class A { B b; } class B { public B() {} private void hidden() {} protected void protectedOne() {} void packageOne() {} public static void staticOne() {} public final void finalOne() {} public synchronized void sync() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        methods(&s, "b"),
        vec!["finalOne", "sync", "equals", "hashCode", "toString"]
    );
}
#[test]
fn owner_superclass_final_methods_are_filtered() {
    let source="package p; public class A extends Parent { B b; } class Parent { public final void ping() {} public void pong() {} } class B { public void ping() {} public void pong() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        methods(&s, "b"),
        vec!["pong", "equals", "hashCode", "toString"]
    );
}
#[test]
fn inherited_fields_are_not_delegation_targets() {
    let source="package p; public class A extends Parent { B own; } class Parent { B inherited; } class B { public void ping() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(fields(&status(&mut ws, &uri, source)), vec!["own"]);
}
#[test]
fn hierarchy_interfaces_and_duplicate_signatures() {
    let source="package p; public class A { B b; } interface I { void ping(); void iface(); } class Parent { public void ping() {} public void parent() {} } class B extends Parent implements I { public void local() {} public void iface() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        methods(&s, "b"),
        vec!["iface", "local", "parent", "ping", "equals", "hashCode", "toString"]
    );
}
#[test]
fn bounded_type_variable_status_and_prompt() {
    let source =
        "package p; public class A<T extends B> { T b; } class B { public void ping() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        methods(&s, "b"),
        vec!["ping", "equals", "hashCode", "toString"]
    );
    assert!(prompt(&actions(&mut ws, &uri, source, None)).is_some());
}
#[test]
fn generic_interface_substitution_and_upper_wildcard_filter() {
    let source="package p; public class A { I<? extends Number> b; } interface I<T> { T get(); void set(T value); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(methods(&s, "b"), vec!["get"]);
}
#[test]
fn lower_wildcard_parameter_is_replaced_by_its_bound() {
    let s = run(
        "package p; public class A { I<? super String> b; } interface I<T> { void set(T value); }",
        &["set"],
    );
    contains(&s, &["public void set(String value)", "b.set(value);"]);
}
#[test]
fn return_covariance_and_exception_rules_preserve_upstream_quirks() {
    let source="package p; public class A { B b; public Object value(){return null;} public void run() throws Exception {} } class B { public String value(){return null;} public void run() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    // Broad owner returns and checked exceptions do not suppress these entries.
    assert!(methods(&s, "b").contains(&"value".into()));
    assert!(methods(&s, "b").contains(&"run".into()));
}
#[test]
fn generic_substitution_and_imported_throws() {
    let s=run("package p; public class A { B<String> b; } class B<T> { public T echo(T value) throws java.io.IOException {return value;} }", &["echo"]);
    contains(
        &s,
        &[
            "import java.io.IOException;",
            "public String echo(String value) throws IOException",
            "return b.echo(value);",
        ],
    );
}
#[test]
fn generic_method_bounds_varargs_and_modifiers() {
    let s=run("package p; public class A { B b; } class B { public final synchronized <T extends Number & Comparable<T>> T first(T... values) throws java.io.IOException {return values[0];} }", &["first"]);
    contains(&s,&["public final <T extends Number & Comparable<T>> T first(T... values) throws IOException","return b.first(values);"]);
    assert!(!s.contains("public final synchronized <T") || s.matches("synchronized").count() == 1);
}
#[test]
fn primitive_return_void_and_array_signatures() {
    let s=run("package p; public class A { B b; } class B { public int size(){return 0;} public void reset(){} public String[][] arrays(int[][] counts){return null;} }", &["size","reset","arrays"]);
    contains(
        &s,
        &[
            "public int size() { return b.size(); }",
            "public void reset() { b.reset(); }",
            "public String[][] arrays(int[][] counts) { return b.arrays(counts); }",
        ],
    );
}
#[test]
fn imports_respect_nested_type_conflicts() {
    let s=run("package p; public class A { B b; class IOException {} } class B { public java.util.List<String> values() throws java.io.IOException {return null;} }", &["values"]);
    contains(
        &s,
        &[
            "import java.util.List;",
            "public List<String> values() throws java.io.IOException",
        ],
    );
    assert!(!s.contains("import java.io.IOException;"));
}
#[test]
fn field_parameter_collision_retains_bare_field_invocation() {
    let s = run(
        "package p; public class A { B b; } class B { public void copy(B b){} }",
        &["copy"],
    );
    contains(&s, &["public void copy(B b) { b.copy(b); }"]);
    assert!(!s.contains("this.b.copy"));
}
#[test]
fn selection_sort_source_order_fields_and_duplicates() {
    let source="package p; public class A { B first; B second; } class B { public void zebra() {} public void alpha() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let a = select(&st, "first", &["alpha"]).remove(0);
    let z = select(&st, "first", &["zebra"]).remove(0);
    let second = select(&st, "second", &["zebra"]).remove(0);
    let s = generate(&mut ws, &uri, source, vec![second, a, z.clone(), z]);
    assert!(s.find("first.zebra").unwrap() < s.find("first.alpha").unwrap());
    assert!(s.find("first.alpha").unwrap() < s.find("second.zebra").unwrap());
    assert_eq!(s.matches("first.zebra()").count(), 2);
}
#[test]
fn jdk_methods_sort_by_attached_source_order() {
    let source = "package p; public class A { String b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let mut es = select(&st, "b", &["toString", "hashCode", "isEmpty"]);
    es.reverse();
    let s = generate(&mut ws, &uri, source, es);
    assert!(s.find("b.isEmpty").unwrap() < s.find("b.hashCode").unwrap());
    assert!(s.find("b.isEmpty").unwrap() < s.find("b.toString").unwrap());
}
#[test]
fn stale_key_is_ignored_when_a_valid_entry_remains() {
    let source = "package p; public class A { B b; } class B { public void ping(){} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let e = select(&st, "b", &["ping"]).remove(0);
    let mut stale = e.clone();
    stale["delegateMethod"]["bindingKey"] = json!("stale");
    let s = generate(&mut ws, &uri, source, vec![stale, e]);
    assert_eq!(s.matches("b.ping()").count(), 1);
}
#[test]
fn empty_and_null_requests_return_null() {
    let source = "package p; public class A { String b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    for entries in [json!([]), Value::Null] {
        assert!(ws
            .request(
                "java/generateDelegateMethods",
                json!({"context":params(&uri,source),"delegateEntries":entries})
            )
            .is_null());
    }
}
#[test]
fn record_components_status_and_prompt() {
    let source = "package p; public record A(B b) {} class B { public void ping(){} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    assert!(methods(&st, "b").contains(&"ping".into()));
    assert!(prompt(&actions(&mut ws, &uri, source, None)).is_none());
}
#[test]
fn capability_and_kind_filtering_and_command_arguments() {
    let source = "package p; public class A { B b; } class B { public void ping(){} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let a = actions(
        &mut ws,
        &uri,
        source,
        Some(vec!["source.generate.delegateMethods"]),
    );
    let p = prompt(&a).expect("prompt");
    assert_eq!(p["title"], "Generate Delegate Methods...");
    let mut expected = params(&uri, source);
    expected["context"]["only"] = json!(["source.generate.delegateMethods"]);
    assert_eq!(p["command"]["arguments"], json!([expected]));
    assert!(p["edit"].is_null());
    assert!(prompt(&actions(&mut ws, &uri, source, Some(vec!["quickassist"]))).is_none());
    assert!(prompt(&actions(
        &mut ws,
        &uri,
        source,
        Some(vec!["source.generate.toString"])
    ))
    .is_none());
}
#[test]
fn unsupported_prompt_capability_hides_source_action() {
    let source = "package p; public class A { String b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(false);
    open(&mut ws, &uri);
    assert!(prompt(&actions(&mut ws, &uri, source, None)).is_none());
}
#[test]
fn command_fallback_without_literal_kind_support() {
    let source = "package p; public class A { String b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, None);
    let p = prompt(&a).expect("command fallback");
    assert_eq!(p["command"], "java.action.generateDelegateMethodsPrompt");
    assert_eq!(p["arguments"], json!([params(&uri, source)]));
    assert!(prompt(&actions(&mut ws, &uri, source, Some(vec!["source"]))).is_none());
}
#[test]
fn default_delegate_comment_uses_erased_declaration_link() {
    let source="package p; public class A { B<String> b; } class B<T> { public T echo(T value) throws java.io.IOException {return value;} }";
    let (mut ws, _, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["echo"]));
    contains(
        &s,
        &[
            "@param value",
            "@return",
            "@throws IOException",
            "@see p.B#echo(java.lang.Object)",
        ],
    );
}
#[test]
fn virtual_documents_support_discovery_prompt_and_generation() {
    if is_oracle() {
        return;
    }
    let source = "package p; public class A { B b; } class B { public void ping(){} }";
    for uri in [
        "untitled:Delegate.java",
        "inmemory://p/A.java",
        "file:///nonexistent-delegate/A.java",
    ] {
        let mut ws = Workspace::new();
        ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
            json!(true);
        ws.open_version(uri, source, 1);
        let st = status(&mut ws, uri, source);
        assert!(methods(&st, "b").contains(&"ping".into()));
        assert!(prompt(&actions(&mut ws, uri, source, None)).is_some());
        let s = generate(&mut ws, uri, source, select(&st, "b", &["ping"]));
        contains(&s, &["public void ping() { b.ping(); }"]);
    }
}
#[test]
fn external_source_methods_sort_by_declaration_order() {
    let source = "package p; public class A { B b; }";
    let (mut ws, root, uri) = setup(source, json!({}));
    ws.create_cu(
        &root,
        "src",
        "p",
        "B.java",
        "package p; public class B { public void zebra(){} public void alpha(){} }",
    );
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let mut entries = select(&st, "b", &["alpha", "zebra"]);
    entries.reverse();
    let s = generate(&mut ws, &uri, source, entries);
    assert!(s.find("b.zebra").unwrap() < s.find("b.alpha").unwrap());
}
#[test]
fn unbounded_type_variable_includes_object_methods() {
    let source = "package p; public class A<T> { T b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(
        methods(&status(&mut ws, &uri, source), "b"),
        vec!["equals", "hashCode", "toString"]
    );
}
#[test]
fn intersection_type_bounds_share_signature_exclusions() {
    let source="package p; public class A<T extends I & J> { T b; } interface I { void ping(); void first(); } interface J { void ping(); void second(); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(
        methods(&status(&mut ws, &uri, source), "b"),
        vec!["first", "ping", "second"]
    );
}
#[test]
fn enum_and_record_synthetic_methods_differ() {
    let source="package p; public record A(B b) {} class B { public int hashCode(){return 0;} public boolean equals(Object o){return false;} public String toString(){return null;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(
        methods(&status(&mut ws, &uri, source), "b"),
        vec!["equals", "hashCode", "toString"]
    );
}
#[test]
fn prompt_survives_empty_discovery_on_reference_fields() {
    let source =
        "package p; public class A { I b; public void ping(){} } interface I { void ping(); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(status(&mut ws, &uri, source), json!({"delegateFields":[]}));
    assert!(prompt(&actions(&mut ws, &uri, source, None)).is_some());
}
#[test]
fn static_record_field_can_offer_prompt() {
    let source = "package p; public record A(int n) { static String b; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert!(prompt(&actions(&mut ws, &uri, source, None)).is_some());
}
#[test]
fn argument_preferences_and_keyword_this_do_not_change_field_expression() {
    let source="package p; public class A { B b; } class B { public void send(String text, String pOther){} }";
    let (mut ws, root, uri) = setup(source, json!({}));
    let prefs = root.join(".settings/org.eclipse.jdt.core.prefs");
    let mut text = std::fs::read_to_string(&prefs).unwrap();
    text.push_str("\norg.eclipse.jdt.core.codeComplete.argumentPrefixes=p\n");
    std::fs::write(prefs, text).unwrap();
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        "org.eclipse.jdt.ui.keywordthis=true\n",
    )
    .unwrap();
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["send"]));
    contains(
        &s,
        &["public void send(String pText, String pOther) { b.send(pText, pOther); }"],
    );
}
#[test]
fn custom_delegate_template_receives_see_target_and_tags() {
    let source="package p; public class A { B b; } class B { @Deprecated public String echo(String text) throws java.io.IOException {return text;} }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    let xml = r#"<templates><template id="org.eclipse.jdt.ui.text.codetemplates.delegatecomment" name="delegatecomment" description="delegatecomment" context="delegatecomment_context" enabled="true" deleted="false" autoinsert="true">/**&#10; * Delegate ${enclosing_method} (${return_type}) in ${file_name}&#10; * ${tags}&#10; * ${see_to_target}&#10; */</template></templates>"#;
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!("org.eclipse.jdt.ui.text.custom_code_templates={xml}\n"),
    )
    .unwrap();
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["echo"]));
    contains(
        &s,
        &[
            "Delegate echo (String) in A.java",
            "@param text",
            "@return",
            "@throws IOException",
            "@deprecated",
            "@see p.B#echo(java.lang.String)",
        ],
    );
}
#[test]
fn header_before_cursor_inserts_before_first_member() {
    let source="package p;\npublic class A {\n    B b;\n    void existing() {}\n}\nclass B { public void ping() {} }";
    let (mut ws, _, uri) = setup(
        source,
        json!({"java.codeGeneration.insertionLocation":"beforeCursor"}),
    );
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["ping"]));
    assert!(s.find("public void ping").unwrap() < s.find("B b;").unwrap());
}
#[test]
fn all_stale_keys_report_internal_error() {
    let source = "package p; public class A { B b; } class B { public void ping(){} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let mut e = select(&st, "b", &["ping"]).remove(0);
    e["delegateMethod"]["bindingKey"] = json!("stale");
    let response = ws.client().request_response(
        "java/generateDelegateMethods",
        json!({"context":params(&uri,source),"delegateEntries":[e]}),
    );
    assert_eq!(response["error"]["code"], -32603, "{response}");
}
#[test]
fn secondary_external_source_type_preserves_method_source_order() {
    let source = "package p; public class A { C b; }";
    let (mut ws, root, uri) = setup(source, json!({}));
    let other = ws.create_cu(
        &root,
        "src",
        "p",
        "B.java",
        "package p; public class B {} class C { public void zebra(){} public void alpha(){} }",
    );
    open(&mut ws, &other);
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let mut entries = select(&st, "b", &["alpha", "zebra"]);
    assert_eq!(entries.len(), 2, "{st}");
    entries.reverse();
    let s = generate(&mut ws, &uri, source, entries);
    assert!(s.find("b.zebra").unwrap() < s.find("b.alpha").unwrap());
}
#[test]
fn empty_default_comment_tags_preserve_first_blank_line() {
    let source = "package p; public class A { B b; } class B { public void ping() {} }";
    let (mut ws, _, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["ping"]));
    assert!(
        s.contains("/**\n     * \n     * @see p.B#ping()\n     */"),
        "{s}"
    );
}
#[test]
fn comment_without_tags_preserves_intentional_blank_line() {
    let source = "package p; public class A { B b; } class B { public void ping() {} }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    let xml = r#"<templates><template id="org.eclipse.jdt.ui.text.codetemplates.delegatecomment" name="delegatecomment" description="delegatecomment" context="delegatecomment_context" enabled="true" deleted="false" autoinsert="true">/**&#10; * Delegate&#10; *&#10; * ${see_to_target}&#10; */</template></templates>"#;
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!("org.eclipse.jdt.ui.text.custom_code_templates={xml}\n"),
    )
    .unwrap();
    open(&mut ws, &uri);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["ping"]));
    assert!(
        s.contains("* Delegate\n     *\n     * @see p.B#ping()"),
        "{s}"
    );
}
#[test]
fn utf16_crlf_open_buffer_changes_keep_disk_untouched() {
    let disk = "package p;\r\npublic class A { String old; }";
    let source="package p;\r\npublic class A { String emoji=\"😀\"; B b; }\r\nclass B { public void ping() {} }";
    let (mut ws, root, uri) = setup(disk, json!({}));
    open(&mut ws, &uri);
    ws.change(&uri, source);
    let st = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, "b", &["ping"]));
    contains(&s, &["emoji=\"😀\"", "public void ping() { b.ping(); }"]);
    assert!(s.contains("\r\n"));
    assert_eq!(
        std::fs::read_to_string(root.join("src/p/A.java")).unwrap(),
        disk
    );
}
#[test]
fn interface_and_array_covariant_returns_suppress_existing_signatures() {
    let source="package p; public class A { B b; public I value(){return null;} public String[] items(){return null;} } interface I {} class B { public Object value(){return null;} public Object[] items(){return null;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert_eq!(
        methods(&status(&mut ws, &uri, source), "b"),
        vec!["equals", "hashCode", "toString"]
    );
}
