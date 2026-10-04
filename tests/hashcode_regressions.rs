//! HashCode/equals cases beyond the sixteen upstream ports.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::PathBuf;
fn setup(source: &str, settings: Value) -> (Workspace, PathBuf, String) {
    let mut ws = Workspace::new();
    ws.settings = settings;
    let mut options = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    let root = ws.new_empty_project(&options);
    let file = if source.contains("public class Host") {
        "Host.java"
    } else {
        "A.java"
    };
    let uri = ws.create_cu(&root, "src", "p", file, source);
    (ws, root, uri)
}
fn params(uri: &str, source: &str) -> Value {
    let token = if source.contains("public class A") {
        "public class A"
    } else if source.contains("class A {") {
        "class A {"
    } else if source.contains("record A") {
        "record A"
    } else {
        "A"
    };
    json!({"textDocument":{"uri":uri},"range":get_range(source,token),"context":{"diagnostics":[]}})
}
fn open(ws: &mut Workspace, uri: &str) {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
}
fn status(ws: &mut Workspace, uri: &str, source: &str) -> Value {
    ws.request("java/checkHashCodeEqualsStatus", params(uri, source))
}
fn chosen(s: &Value, names: &[&str]) -> Value {
    json!(s["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| names.contains(&f["name"].as_str().unwrap()))
        .cloned()
        .collect::<Vec<_>>())
}
fn generate(
    ws: &mut Workspace,
    uri: &str,
    source: &str,
    fields: Value,
    regenerate: bool,
) -> String {
    let edit = ws.request(
        "java/generateHashCodeEquals",
        json!({"context":params(uri,source),"fields":fields,"regenerate":regenerate}),
    );
    assert!(edit["documentChanges"].is_null(), "{edit}");
    apply_edits(
        source,
        edit["changes"][uri]
            .as_array()
            .expect("hashCode/equals changes"),
    )
}
fn run(source: &str, settings: Value, names: &[&str]) -> String {
    let (mut ws, _, uri) = setup(source, settings);
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    generate(&mut ws, &uri, source, chosen(&s, names), false)
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, expected: &[&str]) {
    for e in expected {
        assert!(compact(s).contains(&compact(e)), "missing {e:?} in {s}");
    }
}
#[test]
fn status_exact_keys_fragments_transient_and_inherited_exclusion() {
    let source="package p; class Parent { int inherited; } public class A extends Parent { int one, two; transient String cache; static int skip; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(
        s,
        json!({"type":"A","fields":[{"bindingKey":"Lp/A;.one)I","name":"one","type":"int","isField":true,"isSelected":false},{"bindingKey":"Lp/A;.two)I","name":"two","type":"int","isField":true,"isSelected":false},{"bindingKey":"Lp/A;.cache)Ljava/lang/String;","name":"cache","type":"String","isField":true,"isSelected":false}],"existingMethods":[]})
    );
}
#[test]
fn existing_signature_checks_object_type_not_return_type() {
    let source="package p; public class A { int n; String hashCode(){return null;} int equals(Object a){return 1;} boolean equals(A a){return true;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    assert_eq!(s["existingMethods"], json!(["equals", "hashCode"]));
}
#[test]
fn all_primitive_hash_and_comparison_rules() {
    let source="package p; public class A { boolean b; byte by; short sh; char c; int i; long l; float f; double d; double d2; }";
    let s = run(
        source,
        json!({}),
        &["b", "by", "sh", "c", "i", "l", "f", "d", "d2"],
    );
    contains(
        &s,
        &[
            "(b ? 1231 : 1237)",
            "result = prime * result + by;",
            "result = prime * result + sh;",
            "result = prime * result + c;",
            "result = prime * result + i;",
            "(int) (l ^ (l >>> 32))",
            "Float.floatToIntBits(f)",
            "temp = Double.doubleToLongBits(d);",
            "temp = Double.doubleToLongBits(d2);",
            "Float.floatToIntBits(f) != Float.floatToIntBits(other.f)",
            "Double.doubleToLongBits(d) != Double.doubleToLongBits(other.d)",
        ],
    );
    assert_eq!(s.matches("long temp;").count(), 1);
}
#[test]
fn standalone_objects_hash_boxes_primitives() {
    let s = run(
        "package p; public class A { int n; boolean b; String text; }",
        json!({"java.codeGeneration.hashCodeEquals.useJava7Objects":true}),
        &["n", "b", "text"],
    );
    contains(
        &s,
        &[
            "return Objects.hash(Integer.valueOf(n), Boolean.valueOf(b), text);",
            "return n == other.n && b == other.b && Objects.equals(text, other.text);",
        ],
    );
    assert!(!s.contains("final int prime"));
}
#[test]
fn array_type_controls_deep_hash_and_deep_equals() {
    let s=run("package p; public class A { int[] ints; String[] strings; Object[] objects; Cloneable[] clones; java.io.Serializable[] serials; int[][] matrix; }",json!({}),&["ints","strings","objects","clones","serials","matrix"]);
    contains(
        &s,
        &[
            "Arrays.hashCode(ints)",
            "Arrays.hashCode(strings)",
            "Arrays.deepHashCode(objects)",
            "Arrays.deepHashCode(clones)",
            "Arrays.deepHashCode(serials)",
            "Arrays.deepHashCode(matrix)",
            "Arrays.equals(ints, other.ints)",
            "Arrays.equals(strings, other.strings)",
            "Arrays.deepEquals(objects, other.objects)",
            "Arrays.deepEquals(matrix, other.matrix)",
        ],
    );
}
#[test]
fn objects_hybrid_hash_moves_arrays_before_non_arrays() {
    let s = run(
        "package p; public class A { int n; int[] first; String text; Object[] second; }",
        json!({"java.codeGeneration.hashCodeEquals.useJava7Objects":true}),
        &["n", "first", "text", "second"],
    );
    contains(&s,&["result = prime * result + Arrays.hashCode(first);", "result = prime * result + Arrays.deepHashCode(second);", "result = prime * result + Objects.hash(n, text);", "return n == other.n && Arrays.equals(first, other.first) && Objects.equals(text, other.text) && Arrays.deepEquals(second, other.second);"]);
    assert!(s.find("Arrays.hashCode(first)").unwrap() < s.find("Objects.hash(n, text)").unwrap());
}
#[test]
fn enums_compare_by_identity() {
    let s = run(
        "package p; public class A { enum Kind { ONE } Kind kind; }",
        json!({}),
        &["kind"],
    );
    contains(
        &s,
        &[
            "((kind == null) ? 0 : kind.hashCode())",
            "if (kind != other.kind)",
        ],
    );
    assert!(!s.contains("kind.equals"));
}
#[test]
fn concrete_superclass_methods_are_called() {
    let source="package p; class Parent { public int hashCode(){return 7;} public boolean equals(Object o){return true;} } public class A extends Parent { int n; }";
    let s = run(
        source,
        json!({"java.codeGeneration.hashCodeEquals.useJava7Objects":true}),
        &["n"],
    );
    contains(
        &s,
        &[
            "int result = super.hashCode();",
            "result = prime * result + Objects.hash(n);",
            "if (!super.equals(obj))",
        ],
    );
}
#[test]
fn abstract_superclass_methods_are_not_called() {
    let source="package p; abstract class Parent { public abstract int hashCode(); public abstract boolean equals(Object o); } public class A extends Parent { int n; }";
    let s = run(source, json!({}), &["n"]);
    contains(&s, &["int result = 1;"]);
    assert!(!s.contains("super.hashCode()"));
    assert!(!s.contains("super.equals(obj)"));
}
#[test]
fn binary_superclass_methods_are_called() {
    let s = run(
        "package p; public class A extends java.util.ArrayList<String> { int n; }",
        json!({}),
        &["n"],
    );
    contains(
        &s,
        &["int result = super.hashCode();", "if (!super.equals(obj))"],
    );
}
#[test]
fn nonstatic_member_includes_enclosing_instance_and_helper() {
    let s = run(
        "package p; public class Host { class A { int n; } }",
        json!({}),
        &["n"],
    );
    contains(
        &s,
        &[
            "result = prime * result + getEnclosingInstance().hashCode();",
            "if (!getEnclosingInstance().equals(other.getEnclosingInstance()))",
            "private Host getEnclosingInstance()",
            "return Host.this;",
        ],
    );
}
#[test]
fn static_member_omits_enclosing_instance() {
    let s = run(
        "package p; public class Host { static class A { int n; } }",
        json!({}),
        &["n"],
    );
    assert!(!s.contains("getEnclosingInstance"));
    contains(
        &s,
        &["public int hashCode()", "public boolean equals(Object obj)"],
    );
}
#[test]
fn existing_enclosing_helper_is_preserved() {
    let s=run("package p; public class Host { class A { int n; private Host getEnclosingInstance(){return Host.this;} } }",json!({}),&["n"]);
    assert_eq!(s.matches("private Host getEnclosingInstance()").count(), 1);
    contains(&s, &["getEnclosingInstance().hashCode()"]);
}
#[test]
fn selected_binding_keys_ignore_request_order_metadata_and_duplicates() {
    let source = "package p; public class A { int one; int two; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = status(&mut ws, &uri, source);
    let mut fields = s["fields"].as_array().unwrap().clone();
    fields.reverse();
    fields.push(fields[0].clone());
    for f in &mut fields {
        f["name"] = json!("bad");
        f["isSelected"] = json!(true);
    }
    let s = generate(&mut ws, &uri, source, json!(fields), false);
    assert!(
        s.find("result = prime * result + one;").unwrap()
            < s.find("result = prime * result + two;").unwrap()
    );
    assert!(!s.contains("bad"));
    assert_eq!(s.matches("result = prime * result + two;").count(), 1);
}
#[test]
fn unknown_or_empty_selection_still_generates_both_methods() {
    let source = "package p; public class A { int n; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let s = generate(
        &mut ws,
        &uri,
        source,
        json!([{"bindingKey":"unknown"}]),
        false,
    );
    contains(
        &s,
        &[
            "return super.hashCode();",
            "if (getClass() != obj.getClass())",
            "return true;",
        ],
    );
    assert!(!s.contains("A other"));
}
#[test]
fn regenerate_false_keeps_existing_and_inserts_duplicates() {
    let source="package p; public class A { int n; public int hashCode(){return 123;} public boolean equals(Object o){return false;} }";
    let s = run(source, json!({}), &["n"]);
    assert_eq!(s.matches("public int hashCode()").count(), 2);
    contains(&s, &["return 123;"]);
    assert_eq!(s.matches("public boolean equals(").count(), 2);
}
#[test]
fn regenerate_preserves_overloads_and_replaces_in_place() {
    let source="package p; public class A { int n; public boolean equals(Object o){return false;} String marker=\"marker\"; public int hashCode(){return 123;} public boolean equals(A a){return true;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["n"]), true);
    assert_eq!(s.matches("public int hashCode()").count(), 1);
    assert!(!s.contains("return 123;"));
    contains(&s, &["public boolean equals(A a){return true;}"]);
    assert!(
        s.find("public boolean equals(Object obj)").unwrap() < s.find("String marker").unwrap()
    );
    assert!(s.find("String marker").unwrap() < s.find("public int hashCode()").unwrap());
}
#[test]
fn partial_regenerate_inserts_hash_before_replaced_equals() {
    let source="package p; public class A { int n; public boolean equals(Object o){return false;} String marker=\"marker\"; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["n"]), true);
    assert!(
        s.find("public int hashCode()").unwrap()
            < s.find("public boolean equals(Object obj)").unwrap()
    );
    assert!(
        s.find("public boolean equals(Object obj)").unwrap() < s.find("String marker").unwrap()
    );
}
#[test]
fn local_name_collisions_qualify_fields_in_each_method() {
    let s = run(
        "package p; public class A { int prime; int result; double temp; int obj; int other; }",
        json!({}),
        &["prime", "result", "temp", "obj", "other"],
    );
    contains(
        &s,
        &[
            "result = prime * result + this.prime;",
            "result = prime * result + this.result;",
            "temp = Double.doubleToLongBits(this.temp);",
            "if (this.obj != other.obj)",
            "if (this.other != other.other)",
        ],
    );
}
#[test]
fn import_scope_conflicts_use_qualified_helpers() {
    let source="package p; public class A { static class Arrays {} static class Objects {} static class Float {} int[] array; float f; String text; }";
    let s = run(
        source,
        json!({"java.codeGeneration.hashCodeEquals.useJava7Objects":true}),
        &["array", "f", "text"],
    );
    contains(
        &s,
        &[
            "java.util.Arrays.hashCode(array)",
            "java.util.Objects.hash(f, text)",
            "java.lang.Float.floatToIntBits(f)",
        ],
    );
    assert!(!s.contains("import java.util.Arrays"));
    assert!(!s.contains("import java.util.Objects"));
}
#[test]
fn records_have_component_fields_and_implicit_existing_methods() {
    let source = "package p; public record A(int n, String text) {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    assert_eq!(d["fields"].as_array().unwrap().len(), 2);
    assert_eq!(d["existingMethods"].as_array().unwrap().len(), 2);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["n", "text"]), true);
    contains(
        &s,
        &[
            "public int hashCode()",
            "public boolean equals(Object obj)",
            "if (n != other.n)",
        ],
    );
}
#[test]
fn utf16_crlf_open_buffer_edits_leave_disk_unchanged() {
    let saved = "package p; public class A { int saved; }";
    let source = "package p;\r\n// 😀\r\npublic class A {\r\n int live;\r\n}\r\n";
    let (mut ws, _, uri) = setup(saved, json!({}));
    ws.open_with(&uri, source);
    let d = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["live"]), false);
    contains(&s, &["// 😀", "result = prime * result + live;"]);
    assert!(s.contains("\r\n"));
    assert!(!s.contains("saved"));
    let path = tower_lsp::lsp_types::Url::parse(&uri)
        .unwrap()
        .to_file_path()
        .unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), saved);
}
#[test]
fn virtual_open_documents() {
    if is_oracle() {
        return;
    }
    let source = "public class A { int n; }";
    for uri in [
        "untitled:HashCode.java",
        "inmemory:///A.java",
        "file:///tmp/nonexistent-hashcode/A.java",
    ] {
        let mut ws = Workspace::new();
        ws.open_with(uri, source);
        let d = status(&mut ws, uri, source);
        let s = generate(&mut ws, uri, source, chosen(&d, &["n"]), false);
        contains(&s, &["result = prime * result + n;", "if (n != other.n)"]);
    }
}
fn actions(ws: &mut Workspace, uri: &str, source: &str, only: &str) -> Vec<Value> {
    let mut p = params(uri, source);
    if !only.is_empty() {
        p["context"]["only"] = json!([only]);
    }
    ws.request("textDocument/codeAction", p)
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| {
            a["title"]
                .as_str()
                .is_some_and(|s| s.starts_with("Generate hashCode() and equals()"))
        })
        .cloned()
        .collect()
}
#[test]
fn prompt_capability_kind_filter_and_original_arguments() {
    let source = "package p; public class A { int n; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    assert!(actions(&mut ws, &uri, source, "").is_empty());
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, "");
    assert_eq!(a.len(), 2);
    for a in a {
        assert_eq!(a["command"]["command"], "java.action.hashCodeEqualsPrompt");
        assert_eq!(a["command"]["arguments"][0], params(&uri, source));
        assert!(a["edit"].is_null());
    }
    assert_eq!(
        actions(&mut ws, &uri, source, "source.generate.hashCodeEquals").len(),
        1
    );
    assert!(actions(&mut ws, &uri, source, "quickassist").is_empty());
}
#[test]
fn unsupported_literal_kind_returns_prompt_command() {
    let source = "package p; public class A { int n; }";
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, "");
    assert!(!a.is_empty());
    for a in a {
        assert_eq!(a["command"], "java.action.hashCodeEqualsPrompt");
        assert!(a["kind"].is_null());
    }
    assert!(actions(&mut ws, &uri, source, "source.generate.hashCodeEquals").is_empty());
}
#[test]
fn action_signature_quirk_suppresses_quick_assist_for_other_overload() {
    let source="package p; public class A { int n; public int hashCode(){return 1;} public boolean equals(A a){return true;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    ws.init_options["extendedClientCapabilities"]["hashCodeEqualsPromptSupport"] = json!(true);
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    assert_eq!(d["existingMethods"], json!(["hashCode"]));
    let a = actions(&mut ws, &uri, source, "");
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0]["kind"], "source.generate.hashCodeEquals");
}
fn comment_template(root: &std::path::Path, pattern: &str, extras: &str) {
    let xml=format!("<templates><template id=\"org.eclipse.jdt.ui.text.codetemplates.overridecomment\" name=\"overridecomment\" description=\"overridecomment\" context=\"overridecomment_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{pattern}</template></templates>");
    std::fs::write(root.join(".settings/org.eclipse.jdt.ls.core.prefs"),format!("eclipse.preferences.version=1\n{extras}\norg.eclipse.jdt.ui.text.custom_code_templates={}\n",xml.replace('\\',"\\\\").replace('\n',"\\n"))).unwrap();
}
#[test]
fn override_comment_variables_and_object_see_tags() {
    let source = "package p; public class A { int n; }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    comment_template(&root,"/** ${enclosing_type} ${enclosing_method} ${file_name} ${package_name} ${project_name}\n * ${see_to_overridden}\n */","");
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["n"]), false);
    contains(
        &s,
        &[
            "p.A hashCode A.java p TestProject",
            "p.A equals A.java p TestProject",
            "@see java.lang.Object#hashCode()",
            "@see java.lang.Object#equals(java.lang.Object)",
        ],
    );
    assert!(!s.contains("${"));
}
#[test]
fn java23_markdown_setting_retains_ordinary_override_template() {
    let source = "package p; public class A { int n; }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    let path = root.join(".settings/org.eclipse.jdt.core.prefs");
    let prefs = std::fs::read_to_string(&path)
        .unwrap()
        .replace("=21", "=23");
    std::fs::write(path, prefs).unwrap();
    comment_template(
        &root,
        "/** Ordinary ${enclosing_method} */",
        "org.eclipse.jdt.ui.usemarkdown=true",
    );
    open(&mut ws, &uri);
    let d = status(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&d, &["n"]), false);
    contains(&s, &["/** Ordinary equals */", "/** Ordinary hashCode */"]);
}
#[test]
fn generic_type_and_type_parameter_import_conflict() {
    let s = run(
        "package p; public class A<Objects> { Objects value; }",
        json!({"java.codeGeneration.hashCodeEquals.useJava7Objects":true,"java.codeGeneration.hashCodeEquals.useInstanceof":true}),
        &["value"],
    );
    contains(
        &s,
        &[
            "if (!(obj instanceof A))",
            "A other = (A) obj;",
            "return java.util.Objects.hash(value);",
            "return java.util.Objects.equals(value, other.value);",
        ],
    );
}
#[test]
fn local_type_omits_member_enclosing_helper() {
    let s = run(
        "package p; public class Host { void method() { class A { int n; } } }",
        json!({}),
        &["n"],
    );
    contains(
        &s,
        &[
            "result = prime * result + n;",
            "public boolean equals(Object obj)",
        ],
    );
    assert!(!s.contains("getEnclosingInstance"));
}
