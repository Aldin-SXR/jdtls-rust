//! Constructor protocol and generation cases beyond the thirteen upstream tests.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn setup(
    source: &str,
    preferences: &str,
    options: &[(&str, &str)],
) -> (Workspace, PathBuf, String) {
    let mut ws = Workspace::new();
    let mut opts = test_default_options();
    for (key, value) in options {
        opts.insert((*key).into(), (*value).into());
    }
    let root = ws.new_empty_project(&opts);
    std::fs::write(
        root.join(".settings/org.eclipse.jdt.ls.core.prefs"),
        format!("eclipse.preferences.version=1\n{preferences}"),
    )
    .unwrap();
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    (ws, root, uri)
}
fn open(ws: &mut Workspace, uri: &str) {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
}
fn params(uri: &str, source: &str, token: &str) -> Value {
    json!({"textDocument":{"uri":uri}, "range":get_range(source,token), "context":{"diagnostics":[]}})
}
fn discover(ws: &mut Workspace, uri: &str, source: &str, token: &str) -> Value {
    ws.request("java/checkConstructorsStatus", params(uri, source, token))
}
fn generate(ws: &mut Workspace, uri: &str, source: &str, token: &str, status: &Value) -> String {
    let edit=ws.request("java/generateConstructors", json!({"context":params(uri,source,token),"constructors":status["constructors"],"fields":status["fields"]}));
    assert!(edit["documentChanges"].is_null(), "{edit}");
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .expect("constructor workspace changes"),
    )
}
fn signatures(status: &Value) -> Vec<Value> {
    status["constructors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["parameters"].clone())
        .collect()
}
fn field_names(status: &Value) -> Vec<&str> {
    status["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect()
}
fn constructor_actions(
    ws: &mut Workspace,
    uri: &str,
    source: &str,
    token: &str,
    only: &str,
) -> Vec<Value> {
    actions(ws, uri, source, token, only)
        .into_iter()
        .filter(|a| {
            a["title"]
                .as_str()
                .is_some_and(|s| s.starts_with("Generate Constructors"))
        })
        .collect()
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(actual: &str, fragments: &[&str]) {
    for s in fragments {
        assert!(
            compact(actual).contains(&compact(s)),
            "missing {s:?} in {actual}"
        );
    }
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, token: &str, only: &str) -> Vec<Value> {
    let mut p = params(uri, source, token);
    if !only.is_empty() {
        p["context"]["only"] = json!([only]);
    }
    ws.request("textDocument/codeAction", p)
        .as_array()
        .unwrap()
        .clone()
}
fn templates(root: &Path, values: &[(&str, &str)], prefs: &str) {
    let mut xml = String::from("<templates>");
    for (key, value) in values {
        let escaped = value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        xml.push_str(&format!("<template id=\"org.eclipse.jdt.ui.text.codetemplates.{key}\" name=\"{key}\" description=\"{key}\" context=\"{key}_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{escaped}</template>"));
    }
    xml.push_str("</templates>");
    std::fs::write(root.join(".settings/org.eclipse.jdt.ls.core.prefs"),format!("eclipse.preferences.version=1\n{prefs}\norg.eclipse.jdt.ui.text.custom_code_templates={}\n",xml.replace('\\',"\\\\").replace('\r',"\\r").replace('\n',"\\n"))).unwrap();
}

#[test]
fn status_exact_binding_keys_and_selected_fragments() {
    let source = "package p; public class A {
 int one, two;
 final String initialized=\"x\", pending;
 static int skip;
 A(int already) {}
}";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "two");
    assert_eq!(
        s["constructors"],
        json!([{"bindingKey":"Ljava/lang/Object;.()V","name":"Object","parameters":[]}])
    );
    assert_eq!(
        s["fields"],
        json!([
           {"bindingKey":"Lp/A;.one)I","name":"one","type":"int","isField":true,"isSelected":false},
           {"bindingKey":"Lp/A;.two)I","name":"two","type":"int","isField":true,"isSelected":true},
           {"bindingKey":"Lp/A;.pending)Ljava/lang/String;","name":"pending","type":"String","isField":true,"isSelected":false}
        ])
    );
}
#[test]
fn visibility_across_packages_and_private_fallback() {
    let source = "package p; public class A extends q.Parent {}";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.create_cu(&root,"src","q","Parent.java","package q; public class Parent { public Parent(int x){} protected Parent(String y){} Parent(boolean z){} private Parent(){} }");
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    assert_eq!(signatures(&s), vec![json!(["int"]), json!(["String"])]);
    let changed = "package q; public class A extends Parent {}";
    let other = ws.create_cu(&root, "src", "q", "A.java", changed);
    open(&mut ws, &other);
    assert_eq!(
        signatures(&discover(&mut ws, &other, changed, "class A")),
        vec![json!(["int"]), json!(["String"]), json!(["boolean"])]
    );
    let hidden = ws.create_cu(
        &root,
        "src",
        "p",
        "Hidden.java",
        "package p; class Hidden { private Hidden(int x){} }",
    );
    open(&mut ws, &hidden);
    let changed = "package p; public class A extends Hidden {}";
    ws.change(&uri, changed);
    ws.diagnostics(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let s = discover(&mut ws, &uri, changed, "class A");
    assert_eq!(s["constructors"][0]["name"], "Object");
    contains(
        &generate(&mut ws, &uri, changed, "class A", &s),
        &["public A() {}"],
    );
}
#[test]
fn generic_superclass_type_parameters_throws_and_imports() {
    for bound in ["Number", "Object"] {
        let source="package p; public class A extends Parent<String> { java.util.Map<String,Integer>[] values; }";
        let (mut ws, root, uri) = setup(source, "", &[]);
        ws.create_cu(&root,"src","p","Parent.java",&format!("package p; class Parent<T> {{ public <E extends java.lang.{bound} & Comparable<E>> Parent(T value, E count) throws java.io.IOException {{}} }}"));
        open(&mut ws, &uri);
        let s = discover(&mut ws, &uri, source, "class A");
        assert_eq!(signatures(&s), vec![json!(["String", "E"])]);
        let actual = generate(&mut ws, &uri, source, "class A", &s);
        contains(&actual,&["import java.io.IOException;",&format!("public <E extends {bound} & Comparable<E>> A(String value, E count, Map<String, Integer>[] values) throws IOException"), "super(value, count);", "this.values = values;"]);
    }
}
#[test]
fn varargs_and_field_collision_keep_upstream_parameter_order() {
    let source = "package p; public class A extends Parent { int values; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { Parent(String[]... values){} }",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    assert_eq!(signatures(&s), vec![json!(["String[][]"])]);
    contains(
        &generate(&mut ws, &uri, source, "class A", &s),
        &[
            "public A(String[]... values, int values2)",
            "super(values);",
            "values = values2;",
        ],
    );
}
#[test]
fn field_and_argument_affixes_and_existing_parameter_names() {
    let source = "package p; public class A extends Parent { int mValue_, mOther_; }";
    let opts = [
        ("org.eclipse.jdt.core.codeComplete.fieldPrefixes", "m"),
        ("org.eclipse.jdt.core.codeComplete.fieldSuffixes", "_"),
        ("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p"),
        ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
    ];
    let (mut ws, root, uri) = setup(
        source,
        "org.eclipse.jdt.ui.keywordthis=true
",
        &opts,
    );
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { Parent(int pValueArg, int plain){} }",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    contains(
        &generate(&mut ws, &uri, source, "class A", &s),
        &[
            "public A(int pValueArg, int pPlainArg, int pValue2Arg, int pOtherArg)",
            "super(pValueArg, pPlainArg);",
            "mValue_ = pValue2Arg;",
            "mOther_ = pOtherArg;",
        ],
    );
}
#[test]
fn no_argument_super_constructor_omits_super_throws_and_type_parameters() {
    let source = "package p; public class A extends Parent { int value; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { <T> Parent() throws java.io.IOException {} }",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    let actual = generate(&mut ws, &uri, source, "class A", &s);
    contains(&actual, &["public A(int value) { this.value = value; }"]);
    for excluded in ["super(", "throws", "IOException", "<T>"] {
        assert!(!actual.contains(excluded), "{actual}");
    }
}
#[test]
fn enum_generation_skips_constants_and_initialized_final_fields() {
    let source = "package p; public enum A { FIRST; int value; final int initialized=1; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "enum A");
    assert_eq!(field_names(&s), vec!["value"]);
    contains(
        &generate(&mut ws, &uri, source, "enum A", &s),
        &["private A(int value)", "this.value = value;"],
    );
}
#[test]
fn records_omit_components_and_generate_reference_constructor() {
    let source = "package p; public record A(String name, int age) { static int other; }";
    let opts = [
        ("org.eclipse.jdt.core.compiler.source", "21"),
        ("org.eclipse.jdt.core.compiler.compliance", "21"),
        ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "21"),
    ];
    let (mut ws, _, uri) = setup(source, "", &opts);
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "record A");
    assert!(field_names(&s).is_empty());
    assert_eq!(signatures(&s), vec![json!([])]);
    contains(
        &generate(&mut ws, &uri, source, "record A", &s),
        &["public A() {}"],
    );
}
#[test]
fn nested_and_local_constructor_types_use_the_selected_declaration() {
    let source="package p; public class A { int outer; class Inner { String inner; } void f(){ class Local { int local; } } }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    for (token, name, signature) in [
        ("Inner", "inner", "public Inner(String inner)"),
        ("Local", "local", "public Local(int local)"),
    ] {
        let s = discover(&mut ws, &uri, source, token);
        assert_eq!(field_names(&s), vec![name]);
        let actual = generate(&mut ws, &uri, source, token, &s);
        contains(&actual, &[signature]);
        assert!(!actual.contains("public A("));
    }
}
#[test]
fn binding_keys_choose_fields_but_constructor_signatures_choose_overloads() {
    let source = "package p; public class A extends Parent { int one, two; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { Parent(int x){} Parent(String y){} }",
    );
    open(&mut ws, &uri);
    let mut s = discover(&mut ws, &uri, source, "class A");
    s["constructors"] = json!([{"bindingKey":"ignored","name":"ignored","parameters":["String"]}]);
    s["fields"] = json!([s["fields"][1].clone(),s["fields"][0].clone(),{"bindingKey":"missing","name":"one"}]);
    contains(
        &generate(&mut ws, &uri, source, "class A", &s),
        &[
            "public A(String y, int two, int one)",
            "super(y);",
            "this.two = two;",
            "this.one = one;",
        ],
    );
}
#[test]
fn header_insertion_ignores_last_member_preference_and_follows_last_field() {
    let source = "package p; public class A {
 int first;
 void earlier(){}
 int last;
 void later(){}
}";
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.insertionLocation"] = json!("lastMember");
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "A");
    let actual = generate(&mut ws, &uri, source, "A", &s);
    assert!(actual.find("int last;").unwrap() < actual.find("public A(").unwrap());
    assert!(actual.find("public A(").unwrap() < actual.find("void later()").unwrap());
}
#[test]
fn utf16_crlf_open_buffer_generation_preserves_disk_and_line_endings() {
    let disk = "package p; public class A {}";
    let source = "package p;\r\npublic class A {\r\n // 😀\r\n String name;\r\n}\r\n";
    let (mut ws, root, uri) = setup(disk, "", &[]);
    ws.open_with(&uri, source);
    ws.diagnostics(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let s = discover(&mut ws, &uri, source, "name");
    let actual = generate(&mut ws, &uri, source, "name", &s);
    contains(&actual, &["public A(String name)", "this.name = name;"]);
    assert!(!actual.replace("\r\n", "").contains('\n'));
    assert_eq!(
        std::fs::read_to_string(root.join("src/p/A.java")).unwrap(),
        disk
    );
}
#[test]
fn virtual_documents_support_constructor_protocol() {
    if is_oracle() {
        return;
    } // The oracle requires an ICompilationUnit resource.
    let source = "public class A { int value; }";
    for scheme in ["untitled", "inmemory", "file"] {
        let mut ws = Workspace::new();
        let uri = match scheme {
            "untitled" => "untitled:Constructor.java".to_owned(),
            "inmemory" => "inmemory:///A.java".to_owned(),
            _ => tower_lsp::lsp_types::Url::from_file_path(ws.dir.join("A.java"))
                .unwrap()
                .to_string(),
        };
        ws.open_with(&uri, source);
        let s = discover(&mut ws, &uri, source, "value");
        assert_eq!(field_names(&s), vec!["value"], "{scheme}");
        contains(
            &generate(&mut ws, &uri, source, "class A", &s),
            &["public A(int value)", "this.value = value;"],
        );
        assert!(!ws.dir.join("A.java").exists());
    }
}
#[test]
fn constructor_actions_require_capability_and_respect_kind_filters() {
    let source = "package p; public class A { int value; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    assert!(constructor_actions(&mut ws, &uri, source, "value", "").is_empty());
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.init_options["extendedClientCapabilities"]["generateConstructorsPromptSupport"] =
        json!(true);
    open(&mut ws, &uri);
    let a = constructor_actions(&mut ws, &uri, source, "value", "");
    assert_eq!(a.len(), 2);
    for a in &a {
        assert_eq!(
            a["command"]["command"],
            "java.action.generateConstructorsPrompt"
        );
        assert_eq!(a["command"]["arguments"][0], params(&uri, source, "value"));
    }
    assert_eq!(
        constructor_actions(
            &mut ws,
            &uri,
            source,
            "value",
            "source.generate.constructors"
        )
        .len(),
        1
    );
    assert!(constructor_actions(&mut ws, &uri, source, "value", "quickassist").is_empty());
}
#[test]
fn unsupported_literal_kind_returns_prompt_command() {
    let source = "package p; public class A { int value; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.init_options["extendedClientCapabilities"]["generateConstructorsPromptSupport"] =
        json!(true);
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = constructor_actions(&mut ws, &uri, source, "value", "");
    assert!(!a.is_empty());
    for a in a {
        assert_eq!(a["command"], "java.action.generateConstructorsPrompt");
        assert!(a["kind"].is_null());
    }
    assert!(constructor_actions(
        &mut ws,
        &uri,
        source,
        "value",
        "source.generate.constructors"
    )
    .is_empty());
}
#[test]
fn direct_constructor_actions_are_eager_or_resolved_with_changes() {
    for resolve in [false, true] {
        let source = "package p; public class A {}";
        let (mut ws, _, uri) = setup(source, "", &[]);
        ws.init_options["extendedClientCapabilities"]["generateConstructorsPromptSupport"] =
            json!(true);
        if !resolve {
            ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(false);
        }
        open(&mut ws, &uri);
        let a = constructor_actions(
            &mut ws,
            &uri,
            source,
            "class A",
            "source.generate.constructors",
        );
        assert_eq!(a.len(), 1, "{a:?}");
        let action = if resolve {
            assert!(a[0]["edit"].is_null());
            assert!(!a[0]["data"].is_null());
            ws.request("codeAction/resolve", a[0].clone())
        } else {
            a[0].clone()
        };
        assert!(action["edit"]["documentChanges"].is_null(), "{action}");
        let actual = apply_edits(source, action["edit"]["changes"][&uri].as_array().unwrap());
        contains(&actual, &["public A() {}"]);
    }
}
#[test]
fn default_constructor_comment_contains_parameter_and_exception_tags() {
    let source = "package p; public class A extends Parent { int value; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { <T> Parent(T input) throws java.io.IOException {} }",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    contains(
        &generate(&mut ws, &uri, source, "class A", &s),
        &[
            "/**",
            "* @param <T>",
            "* @param input",
            "* @param value",
            "* @throws IOException",
        ],
    );
}
#[test]
fn custom_constructor_comments_and_body_template_behavior() {
    let source = "package p; public class A { class Inner { int value; } }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    templates(&root,&[("constructorcomment","/** ${enclosing_type}.${enclosing_method} ${file_name} ${package_name} ${project_name} ${dollar} ${alias:enclosing_type}\n * ${tags}\n */"),("constructorbody","throw new RuntimeException();")],"");
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "Inner");
    contains(
        &generate(&mut ws, &uri, source, "Inner", &s),
        &[
            "/** Inner.Inner A.java p TestProject $ Inner",
            "* @param value",
            "this.value = value;",
        ],
    );
}
#[test]
fn empty_tags_leave_default_empty_javadoc_and_blank_template_omits_comment() {
    let source = "package p; public class A {}";
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    contains(
        &generate(&mut ws, &uri, source, "class A", &s),
        &["/** * */ public A() {}"],
    );
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    templates(&root, &[("constructorcomment", " ")], "");
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "class A");
    assert!(!generate(&mut ws, &uri, source, "class A", &s).contains("/**"));
}
#[test]
fn markdown_constructor_comment_template_and_compliance() {
    for level in ["21", "23"] {
        let source = "package p; public class A { int value; }";
        let opts = [
            ("org.eclipse.jdt.core.compiler.source", level),
            ("org.eclipse.jdt.core.compiler.compliance", level),
            (
                "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
                level,
            ),
        ];
        let (mut ws, root, uri) = setup(source, "", &opts);
        ws.settings["java.codeGeneration.generateComments"] = json!(true);
        templates(
            &root,
            &[(
                "markdownconstructorcomment",
                "/// New ${enclosing_type}\n/// ${tags}",
            )],
            "org.eclipse.jdt.ui.usemarkdown=true",
        );
        open(&mut ws, &uri);
        let s = discover(&mut ws, &uri, source, "class A");
        let actual = generate(&mut ws, &uri, source, "class A", &s);
        if level == "23" {
            contains(&actual, &["/// New A", "/// @param value"]);
            assert!(!actual.contains("/**"));
        } else {
            contains(&actual, &["/**", "* @param value"]);
            assert!(!actual.contains("///"));
        }
    }
}

#[test]
fn imported_types_respect_nested_type_and_import_conflicts() {
    let source="package p; import java.awt.List; public class A { class Map {} java.util.Map<String,Integer> values; java.util.List<String> items; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "A");
    let actual = generate(&mut ws, &uri, source, "A", &s);
    contains(
        &actual,
        &["public A(java.util.Map<String,Integer> values, java.util.List<String> items)"],
    );
    assert!(!actual.contains("import java.util.Map;"));
    assert!(!actual.contains("import java.util.List;"));
}
#[test]
fn comments_repeat_tags_after_expanding_prefix_and_include_deprecation() {
    let source = "package p; public class A extends Parent { int second; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { @Deprecated Parent(String first){} }",
    );
    templates(
        &root,
        &[(
            "constructorcomment",
            "/**\n * ${enclosing_type}: ${tags}\n * Other ${tags}\n * Multiple ${tags} / ${tags}\n */",
        )],
        "",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "A");
    let actual = generate(&mut ws, &uri, source, "A", &s);
    contains(
        &actual,
        &[
            "* A: @param first\n * A: @param second\n * A: @deprecated",
            "* Other @param first\n * Other @param second\n * Other @deprecated",
            "* Multiple @param first\n * Multiple @param second\n * Multiple @deprecated / @param first\n * Multiple @ / @param second\n * Multiple @ / @deprecated",
        ],
    );
}
#[test]
fn selected_constructor_unknown_signature_returns_reference_noop() {
    let source = "package p; public class A { int one; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    for constructors in [Value::Null, json!([])] {
        let edit = ws.request(
            "java/generateConstructors",
            json!({"context":params(&uri,source,"A"),"constructors":constructors,"fields":[]}),
        );
        assert!(edit.is_null(), "{edit}");
    }
    let edit=ws.request("java/generateConstructors",json!({"context":params(&uri,source,"A"),"constructors":[{"parameters":["unknown"]}],"fields":[]}));
    assert_eq!(
        edit["changes"][&uri],
        json!([{ "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"" }])
    );
}
#[test]
fn already_affixed_super_parameters_preserve_the_original_case() {
    let source = "package p; public class A extends Parent { int other; }";
    let (mut ws, root, uri) = setup(
        source,
        "",
        &[
            ("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p"),
            ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
        ],
    );
    ws.create_cu(
        &root,
        "src",
        "p",
        "Parent.java",
        "package p; class Parent { Parent(String pURLArg){} }",
    );
    open(&mut ws, &uri);
    let s = discover(&mut ws, &uri, source, "A");
    contains(
        &generate(&mut ws, &uri, source, "A", &s),
        &["public A(String pURLArg, int pOtherArg)", "super(pURLArg);"],
    );
}
