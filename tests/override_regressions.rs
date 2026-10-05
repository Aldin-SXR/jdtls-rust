//! Override discovery, implementation stubs and action protocol regressions.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::PathBuf;
fn setup(source: &str, settings: Value) -> (Workspace, PathBuf, String) {
    let mut ws = Workspace::new();
    ws.settings = settings;
    ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(true);
    let mut options = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    let root = ws.new_empty_project(&options);
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    (ws, root, uri)
}
fn params(uri: &str, source: &str) -> Value {
    let token = [
        "public class A",
        "public interface A",
        "public record A",
        "public enum A",
    ]
    .into_iter()
    .find(|t| source.contains(t))
    .unwrap_or("class A");
    json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})
}
fn open(ws: &mut Workspace, uri: &str) {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
}
fn list(ws: &mut Workspace, uri: &str, source: &str) -> Value {
    ws.request("java/listOverridableMethods", params(uri, source))
}
fn names(s: &Value) -> Vec<String> {
    s["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().into())
        .collect()
}
fn select(s: &Value, names: &[&str]) -> Vec<Value> {
    s["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| names.contains(&m["name"].as_str().unwrap()))
        .cloned()
        .collect()
}
fn generate(ws: &mut Workspace, uri: &str, source: &str, methods: Vec<Value>) -> String {
    let edit = ws.request(
        "java/addOverridableMethods",
        json!({"context":params(uri,source),"overridableMethods":methods}),
    );
    assert!(edit["documentChanges"].is_null(), "{edit}");
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .unwrap_or_else(|| panic!("{edit}")),
    )
}
fn run(source: &str, names: &[&str]) -> String {
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    generate(&mut ws, &uri, source, select(&st, names))
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, expected: &[&str]) {
    for e in expected {
        assert!(compact(s).contains(&compact(e)), "missing {e:?}: {s}");
    }
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, only: Option<Vec<&str>>) -> Value {
    let mut p = params(uri, source);
    if let Some(only) = only {
        p["context"]["only"] = json!(only);
    }
    ws.request("textDocument/codeAction", p)
}
fn prompts(a: &Value) -> Vec<&Value> {
    a.as_array()
        .unwrap()
        .iter()
        .filter(|a| {
            a["command"]["command"] == "java.action.overrideMethodsPrompt"
                || a["command"] == "java.action.overrideMethodsPrompt"
        })
        .collect()
}
fn template(root: &std::path::Path, key: &str, body: &str) {
    let escaped = body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "&#10;");
    let template_context = if key == "methodbodyalternative" {
        "methodbody"
    } else {
        key
    };
    let xml=format!("<templates><template id=\"org.eclipse.jdt.ui.text.codetemplates.{key}\" name=\"{key}\" description=\"{key}\" context=\"{template_context}_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{escaped}</template></templates>");
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!("org.eclipse.jdt.ui.text.custom_code_templates={xml}\n"),
    )
    .unwrap();
}
#[test]
fn dto_declaring_type_binding_key_and_overloads() {
    let source="package p; public class A implements I {} interface I { void send(String text); void send(int[] values); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    assert_eq!(st["type"], "A");
    let ms = select(&st, &["send"]);
    assert_eq!(ms.len(), 2);
    assert!(ms.contains(&json!({"bindingKey":"Lp/A~I;.send(Ljava/lang/String;)V","name":"send","parameters":["String"],"unimplemented":true,"declaringClass":"p.I","declaringClassType":"interface"})),"{st}");
    assert!(ms.iter().any(|m| m["parameters"] == json!(["int[]"])));
}
#[test]
fn private_static_final_own_and_constructor_exclusions() {
    let source="package p; public class A extends B { public void own() {} } class B { B() {} private void hidden() {} public static void staticOne() {} public final void finalOne() {} public void own() {} protected void protectedOne() {} void packageOne() {} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let ns = names(&list(&mut ws, &uri, source));
    for n in [
        "hidden",
        "staticOne",
        "finalOne",
        "own",
        "B",
        "getClass",
        "wait",
        "notify",
    ] {
        assert!(!ns.contains(&n.into()), "{ns:?}");
    }
    for n in ["protectedOne", "packageOne", "clone", "equals"] {
        assert!(ns.contains(&n.into()), "{ns:?}");
    }
}
#[test]
fn cross_package_visibility() {
    let source = "package p; public class A extends q.B {}";
    let (mut ws, root, uri) = setup(source, json!({}));
    ws.create_cu(&root,"src","q","B.java","package q; public class B { void packageOne() {} protected void protectedOne() {} public void publicOne() {} }");
    open(&mut ws, &uri);
    let ns = names(&list(&mut ws, &uri, source));
    assert!(!ns.contains(&"packageOne".into()));
    assert!(ns.contains(&"protectedOne".into()));
    assert!(ns.contains(&"publicOne".into()));
}
#[test]
fn final_class_method_blocks_interface_slot() {
    let source="package p; public class A extends B implements I {} class B { public final void blocked() {} } interface I { void blocked(); void needed(); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    assert!(select(&st, &["blocked"]).is_empty());
    assert_eq!(select(&st, &["needed"])[0]["unimplemented"], true);
}
#[test]
fn inherited_covariant_and_generic_signatures_are_deduplicated() {
    let source="package p; public class A extends B implements I<String> {} class B { public String get(){return null;} public void set(String value){} } interface I<T> { T get(); void set(T value); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    for n in ["get", "set"] {
        let ms = select(&st, &[n]);
        assert_eq!(ms.len(), 1, "{st}");
        assert_eq!(ms[0]["unimplemented"], false);
        assert_eq!(ms[0]["declaringClassType"], "class");
    }
}
#[test]
fn cloneable_flag_is_inherited_but_noncloneable_is_concrete() {
    for (suffix, expected) in [("", false), (" implements Cloneable", true)] {
        let source = format!("package p; public class A extends B {{}} class B{suffix} {{}}");
        let (mut ws, _, uri) = setup(&source, json!({}));
        open(&mut ws, &uri);
        let st = list(&mut ws, &uri, &source);
        assert_eq!(select(&st, &["clone"])[0]["unimplemented"], expected);
    }
}
#[test]
fn abstract_primitive_reference_and_optional_defaults() {
    let source="package p; public class A implements I {} interface I { boolean bool(); int number(); char character(); Object object(); void run(); java.util.Optional<String> optional(); }";
    let s = run(
        source,
        &["bool", "number", "character", "object", "run", "optional"],
    );
    contains(&s,&["boolean bool() { // TODO Auto-generated method stub return false; }","int number() { // TODO Auto-generated method stub return 0; }","char character() { // TODO Auto-generated method stub return 0; }","Object object() { // TODO Auto-generated method stub return null; }","void run() { // TODO Auto-generated method stub }","Optional<String> optional() { // TODO Auto-generated method stub return Optional.empty(); }"]);
}
#[test]
fn concrete_synchronized_super_call_and_throws_import() {
    let source="package p; public class A extends B {} class B { protected synchronized int send(String text) throws java.io.IOException {return 1;} }";
    let s = run(source, &["send"]);
    contains(
        &s,
        &[
            "import java.io.IOException;",
            "@Override protected synchronized int send(String text) throws IOException",
            "return super.send(text);",
        ],
    );
}
#[test]
fn generic_method_bounds_varargs_and_parameter_names() {
    let source="package p; public class A implements I {} interface I { <T extends Number & Comparable<T>> T send(T value, String... labels) throws java.io.IOException; }";
    let s = run(source, &["send"]);
    contains(&s,&["public <T extends Number & Comparable<T>> T send(T value, String... labels) throws IOException","return null;"]);
}
#[test]
fn direct_and_indirect_interface_default_super_qualifier() {
    let source="package p; public class A implements Direct {} interface Direct extends I {} interface I { default int number(){return 1;} }";
    let s = run(source, &["number"]);
    contains(
        &s,
        &["public int number()", "return Direct.super.number();"],
    );
}
#[test]
fn interface_default_inherited_through_class_uses_plain_super() {
    let source="package p; public class A extends B {} class B implements I {} interface I { default void run() {} }";
    let s = run(source, &["run"]);
    contains(&s, &["public void run()", "super.run();"]);
    assert!(!s.contains("I.super"), "{s}");
}
#[test]
fn interface_abstract_override_uses_default_throwing_body() {
    let source = "package p; public interface A extends I {} interface I { int number(); }";
    let s = run(source, &["number"]);
    contains(
        &s,
        &[
            "@Override default int number()",
            "throw new UnsupportedOperationException(\"Unimplemented method 'number'\");",
        ],
    );
}
#[test]
fn interface_object_methods_are_bodyless_with_nonpublic_annotation_exclusion() {
    let source = "package p; public interface A {}";
    let s = run(source, &["equals", "clone"]);
    contains(
        &s,
        &[
            "@Override boolean equals(Object obj);",
            "Object clone() throws CloneNotSupportedException;",
        ],
    );
    assert!(!compact(&s).contains("@OverrideObjectclone"), "{s}");
    assert!(!s.contains("super."));
}
#[test]
fn compiler_option_disables_interface_override_annotation() {
    let source = "package p; public class A implements I {} interface I { void run(); }";
    let (mut ws, root, uri) = setup(source, json!({}));
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation".into(),"disabled".into());
    ws.set_project_options(&root, &options);
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["run"]));
    assert!(!s.contains("@Override"), "{s}");
    contains(&s, &["public void run()"]);
}
#[test]
fn generate_comments_preference_is_ignored_by_override_handler() {
    let source = "package p; public class A implements I {} interface I { void run(); }";
    let (mut ws, _, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["run"]));
    assert!(!s.contains("/**"), "{s}");
    contains(&s, &["@Override", "// TODO Auto-generated method stub"]);
}
#[test]
fn custom_class_body_template_receives_statement_type_and_method() {
    let source =
        "package p; public class A extends B {} class B { public int number(){return 1;} }";
    let (mut ws, root, uri) = setup(source, json!({}));
    template(
        &root,
        "methodbodyalternative",
        "// ${enclosing_type}.${enclosing_method}\n${body_statement}",
    );
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["number"]));
    contains(&s, &["// A.number", "return super.number();"]);
    assert!(!s.contains("Auto-generated"));
}
#[test]
fn blank_class_body_template_falls_back_to_statement() {
    let source =
        "package p; public class A implements I {} interface I { int number(); void run(); }";
    let (mut ws, root, uri) = setup(source, json!({}));
    template(&root, "methodbodyalternative", " ");
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["number", "run"]));
    contains(
        &s,
        &["public int number(){return 0;}", "public void run(){}"],
    );
}
#[test]
fn interface_body_template_receives_default_statement() {
    let source = "package p; public interface A extends I {} interface I { boolean check(); }";
    let (mut ws, root, uri) = setup(source, json!({}));
    template(&root, "methodbody", "${body_statement}");
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["check"]));
    contains(&s, &["default boolean check(){return false;}"]);
}
#[test]
fn selections_use_binding_keys_deduplicate_and_ignore_request_order() {
    let source =
        "package p; public class A implements I {} interface I { void alpha(); void zebra(); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let mut ms = select(&st, &["alpha", "zebra"]);
    ms.reverse();
    ms[0] = json!({"bindingKey":ms[0]["bindingKey"]});
    ms.push(ms[0].clone());
    let s = generate(&mut ws, &uri, source, ms);
    assert_eq!(s.matches("public void alpha").count(), 1, "{s}");
    assert_eq!(s.matches("public void zebra").count(), 1, "{s}");
    assert!(
        s.find("public void alpha").unwrap() < s.find("public void zebra").unwrap(),
        "{s}"
    );
}
#[test]
fn empty_missing_and_stale_selections() {
    let source = "package p; public class A {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    for methods in [Value::Null, json!([])] {
        assert!(ws
            .request(
                "java/addOverridableMethods",
                json!({"context":params(&uri,source),"overridableMethods":methods})
            )
            .is_null());
    }
    let st = list(&mut ws, &uri, source);
    let mut ms = select(&st, &["equals"]);
    ms[0]["bindingKey"] = json!("stale");
    assert_eq!(generate(&mut ws, &uri, source, ms), source);
}
#[test]
fn record_implicit_methods_allow_explicit_object_overrides() {
    let source = "package p; public record A(int value) {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    for n in ["equals", "hashCode", "toString"] {
        assert_eq!(select(&st, &[n]).len(), 1, "{st}");
    }
}
#[test]
fn enum_final_methods_are_excluded() {
    let source = "package p; public enum A { ONE; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let ns = names(&st);
    assert!(ns.contains(&"toString".into()), "{st}");
    for n in [
        "equals",
        "hashCode",
        "clone",
        "name",
        "ordinal",
        "compareTo",
    ] {
        assert!(!ns.contains(&n.into()), "{st}");
    }
}
#[test]
fn nested_type_response_and_selection() {
    let source =
        "package p; public class A { class Inner implements I {} } interface I { void run(); }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let mut p = params(&uri, source);
    p["range"] = get_range(source, "class Inner");
    let st = ws.request("java/listOverridableMethods", p);
    assert_eq!(st["type"], "A$Inner");
    assert_eq!(select(&st, &["run"])[0]["unimplemented"], true);
}
#[test]
fn open_buffer_unicode_crlf_and_header_insertion() {
    let disk = "package p; public class A {}";
    let source="package p;\r\n// 😀\r\npublic class A implements I {\r\n\tint field;\r\n}\r\ninterface I { void run(); }\r\n";
    let (mut ws, _, uri) = setup(
        disk,
        json!({"java.codeGeneration.insertionLocation":"beforeCursor"}),
    );
    open(&mut ws, &uri);
    ws.change(&uri, source);
    let st = list(&mut ws, &uri, source);
    // A name selection maps to the enclosing declaration and appends even when
    // the insertion preference is beforeCursor.
    let mut p = params(&uri, source);
    p["range"] = get_range(source, "A");
    let edit = ws.request(
        "java/addOverridableMethods",
        json!({"context":p,"overridableMethods":select(&st,&["run"])}),
    );
    let s = apply_edits(source, edit["changes"][&uri].as_array().unwrap());
    assert!(
        s.find("int field;").unwrap() < s.find("public void run").unwrap(),
        "{s}"
    );
    assert!(s.contains("// 😀\r\n"));
    assert!(!s.replace("\r\n", "").contains('\n'), "{s}");
}
#[test]
fn action_kinds_arguments_diagnostics_and_resolve() {
    let source = "package p; public class A {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, None);
    let ps = prompts(&a);
    assert_eq!(ps.len(), 2, "{a}");
    for p in ps {
        assert_eq!(p["title"], "Override/Implement Methods...");
        assert_eq!(p["diagnostics"], json!([]));
        assert_eq!(p["command"]["arguments"], json!([params(&uri, source)]));
        let resolved = ws.request("codeAction/resolve", p.clone());
        assert_eq!(resolved["command"], p["command"]);
    }
    let a = actions(&mut ws, &uri, source, Some(vec!["source.overrideMethods"]));
    assert_eq!(prompts(&a).len(), 1);
    assert_eq!(prompts(&a)[0]["kind"], "source.overrideMethods");
    assert!(prompts(&actions(
        &mut ws,
        &uri,
        source,
        Some(vec!["source.generate.toString"])
    ))
    .is_empty());
}
#[test]
fn prompt_capability_and_legacy_command_fallback() {
    let source = "package p; public class A {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(false);
    open(&mut ws, &uri);
    assert!(prompts(&actions(&mut ws, &uri, source, None)).is_empty());
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, None);
    let ps = prompts(&a);
    assert!(!ps.is_empty(), "{a}");
    assert_eq!(ps[0]["command"], "java.action.overrideMethodsPrompt");
    assert_eq!(ps[0]["arguments"], json!([params(&uri, source)]));
    assert!(prompts(&actions(&mut ws, &uri, source, Some(vec!["source"]))).is_empty());
}
#[test]
fn source_action_is_available_on_interface_and_member_selection() {
    let source = "package p; public interface A {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert!(!prompts(&actions(
        &mut ws,
        &uri,
        source,
        Some(vec!["source.overrideMethods"])
    ))
    .is_empty());
    let source = "package p; public class A { int field; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let mut p = params(&uri, source);
    p["range"] = get_range(source, "int field;");
    let a = ws.request("textDocument/codeAction", p);
    let ps = prompts(&a);
    assert_eq!(ps.len(), 1, "{a}");
    assert_eq!(ps[0]["kind"], "source.overrideMethods");
}
#[test]
fn virtual_documents_support_listing_actions_and_generation() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Override.java",
        "inmemory://parity/Override.java",
        "file:///tmp/jdtls-parity-missing-override/A.java",
    ] {
        let source = "public class A implements I {} interface I { int number(); }";
        let mut ws = Workspace::new();
        ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(true);
        ws.open_with(uri, source);
        let st = list(&mut ws, uri, source);
        assert_eq!(select(&st, &["number"])[0]["unimplemented"], true);
        assert!(!prompts(&actions(&mut ws, uri, source, None)).is_empty());
        let s = generate(&mut ws, uri, source, select(&st, &["number"]));
        contains(&s, &["public int number()", "return 0;"]);
    }
}
#[test]
fn local_and_anonymous_type_names_and_generation() {
    for (source, token, expected) in [
        ("package p; public class A { void outer() { class Inner implements I {} } } interface I { void run(); }", "Inner", "A$Inner"),
        ("package p; public class A { I value = new I() { /*cursor*/ }; } interface I { void run(); }", "/*cursor*/", "A$1"),
    ] {
        let (mut ws,_,uri)=setup(source,json!({}));open(&mut ws,&uri);
        let mut p=params(&uri,source);p["range"]=get_range(source,token);
        let st=ws.request("java/listOverridableMethods",p.clone());
        assert_eq!(st["type"],expected,"{st}");
        let edit=ws.request("java/addOverridableMethods",json!({"context":p,"overridableMethods":select(&st,&["run"])}));
        let s=apply_edits(source,edit["changes"][&uri].as_array().unwrap());contains(&s,&["public void run()"]);
        assert!(s.find("public void run()").unwrap()<s.find("interface I").unwrap(),"{s}");
    }
}
#[test]
fn module_and_package_info_do_not_offer_override_prompts() {
    for (file, source) in [
        ("package-info.java", "/** package */ package p;"),
        ("module-info.java", "module sample {}"),
    ] {
        let (mut ws, root, _) = setup("package p; public class A {}", json!({}));
        let uri = ws.create_cu(
            &root,
            "src",
            if file == "module-info.java" { "" } else { "p" },
            file,
            source,
        );
        open(&mut ws, &uri);
        let p = json!({"textDocument":{"uri":uri},"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"context":{"diagnostics":[],"only":["source.overrideMethods"]}});
        assert!(prompts(&ws.request("textDocument/codeAction", p)).is_empty());
    }
}
#[test]
fn custom_task_tag_and_argument_prefix() {
    let source="package p; public class A extends B {} class B { public int number(int value){return value;} }";
    let (mut ws, root, uri) = setup(source, json!({}));
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.taskTags".into(),
        "FIXME,TODO".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.codeComplete.argumentPrefixes".into(),
        "arg".into(),
    );
    ws.set_project_options(&root, &options);
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["number"]));
    contains(
        &s,
        &[
            "public int number(int argValue)",
            "// FIXME Auto-generated method stub",
            "return super.number(argValue);",
        ],
    );
}
#[test]
fn abstract_void_complete_source() {
    let source =
        "package p;\n\npublic class A implements I {\n}\n\ninterface I {\n    void run();\n}\n";
    let s = run(source, &["run"]);
    let expected="package p;\n\npublic class A implements I {\n\n    @Override\n    public void run() {\n        // TODO Auto-generated method stub\n        \n    }\n}\n\ninterface I {\n    void run();\n}\n";
    assert_eq!(s, expected);
}
#[test]
fn unreferenced_nested_declarations_do_not_conflict_with_imports() {
    let source="package p; public class A implements I {} interface I { void use(q.Inner value); } class B { class Inner {} }";
    let (mut ws, root, uri) = setup(source, json!({}));
    ws.create_cu(
        &root,
        "src",
        "q",
        "Inner.java",
        "package q; public class Inner {}",
    );
    open(&mut ws, &uri);
    let st = list(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, select(&st, &["use"]));
    contains(&s, &["import q.Inner;", "public void use(Inner value)"]);
}
