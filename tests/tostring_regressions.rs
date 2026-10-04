//! Additional toString protocol, template, style, selection and edit regressions.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::{json, Value};
use std::path::PathBuf;
fn setup(source: &str, settings: Value) -> (Workspace, PathBuf, String) {
    let mut ws = Workspace::new();
    ws.settings = settings;
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    let root = ws.new_empty_project(&options);
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    (ws, root, uri)
}
fn params(uri: &str, source: &str) -> Value {
    json!({"textDocument":{"uri":uri},"range":get_range(source,"A"),"context":{"diagnostics":[]}})
}
fn open(ws: &mut Workspace, uri: &str) {
    ws.open(uri);
    ws.request("java/buildWorkspace", json!(false));
}
fn discover(ws: &mut Workspace, uri: &str, source: &str) -> Value {
    ws.request("java/checkToStringStatus", params(uri, source))
}
fn generate(ws: &mut Workspace, uri: &str, source: &str, fields: Value) -> String {
    let edit = ws.request(
        "java/generateToString",
        json!({"context":params(uri,source),"fields":fields}),
    );
    assert!(edit["documentChanges"].is_null(), "{edit}");
    apply_edits(
        source,
        edit["changes"][uri].as_array().expect("toString changes"),
    )
}
fn chosen(status: &Value, names: &[&str]) -> Value {
    json!(status["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| names.contains(&f["name"].as_str().unwrap()))
        .cloned()
        .collect::<Vec<_>>())
}
fn run(source: &str, settings: Value, names: &[&str]) -> String {
    let (mut ws, _, uri) = setup(source, settings);
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    generate(&mut ws, &uri, source, chosen(&status, names))
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, expected: &[&str]) {
    for e in expected {
        assert!(compact(s).contains(&compact(e)), "missing {e:?} in {s}");
    }
}
const SIMPLE: &str = "package p; public class A { int id; String label; }";
#[test]
fn concatenation_style() {
    let s = run(SIMPLE, json!({}), &["id", "label"]);
    contains(
        &s,
        &[
            "@Override",
            "return \"A [id=\" + id + \", label=\" + label + \"]\";",
        ],
    );
}
#[test]
fn builder_style() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_BUILDER"}),
        &["id", "label"],
    );
    contains(
        &s,
        &[
            "StringBuilder builder = new StringBuilder();",
            "builder.append(\"A [id=\");",
            "builder.append(id);",
            "builder.append(\", label=\");",
            "builder.append(label);",
            "builder.append(\"]\");",
            "return builder.toString();",
        ],
    );
}
#[test]
fn chained_builder_style() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_BUILDER_CHAINED"}),
        &["id", "label"],
    );
    contains(&s,&["builder.append(\"A [id=\").append(id).append(\", label=\").append(label).append(\"]\");"]);
}
#[test]
fn format_style_ignores_null_skip() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_FORMAT", "java.codeGeneration.toString.skipNullValues":true}),
        &["id", "label"],
    );
    contains(
        &s,
        &["return String.format(\"A [id=%s, label=%s]\", id, label);"],
    );
    assert!(!s.contains("if ("));
}
#[test]
fn concatenation_null_skip_separator() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.skipNullValues":true}),
        &["id", "label"],
    );
    contains(&s,&["return \"A [id=\" + id + \", \" + (label != null ? \"label=\" + label : \"\") + \"]\";"]);
}
#[test]
fn builder_null_skip_multiple_appends_use_block() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_BUILDER","java.codeGeneration.toString.skipNullValues":true,"java.codeGeneration.useBlocks":false}),
        &["id", "label"],
    );
    contains(
        &s,
        &[
            "if (label != null) {",
            "builder.append(\"label=\");",
            "builder.append(label);",
        ],
    );
}
#[test]
fn chained_null_skip_single_statement_without_block() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_BUILDER_CHAINED","java.codeGeneration.toString.skipNullValues":true,"java.codeGeneration.useBlocks":false}),
        &["id", "label"],
    );
    contains(
        &s,
        &["if (label != null) builder.append(\"label=\").append(label);"],
    );
    assert!(!compact(&s).contains("if(label!=null){"));
}
#[test]
fn limited_list_has_null_guard() {
    let s = run(
        "package p; import java.util.List; public class A { List<String> items; }",
        json!({"java.codeGeneration.toString.limitElements":3}),
        &["items"],
    );
    contains(
        &s,
        &[
            "final int maxLen = 3;",
            "(items != null ? items.subList(0, Math.min(items.size(), maxLen)) : null)",
        ],
    );
    assert!(!s.contains("private String toString"));
}
#[test]
fn collections_and_map_share_iterator_helper_with_lists() {
    let s=run("package p; import java.util.*; public class A { List<String> list; Set<String> set; Map<String,Integer> map; }",json!({"java.codeGeneration.toString.limitElements":2}), &["list","set","map"]);
    contains(
        &s,
        &[
            "toString(list, maxLen)",
            "toString(set, maxLen)",
            "toString(map.entrySet(), maxLen)",
            "private String toString(Collection<?> collection, int maxLen)",
            "Iterator<?> iterator = collection.iterator()",
            "iterator.hasNext() && i < maxLen",
            "builder.append(iterator.next());",
        ],
    );
}
#[test]
fn limited_primitive_and_object_arrays() {
    let s = run(
        "package p; public class A { int[] ints; String[] words; int[][] matrix; }",
        json!({"java.codeGeneration.toString.limitElements":2}),
        &["ints", "words", "matrix"],
    );
    contains(
        &s,
        &[
            "import java.util.Arrays;",
            "Arrays.toString(Arrays.copyOf(ints, Math.min(ints.length, maxLen)))",
            "Arrays.asList(words).subList(0, Math.min(words.length, maxLen))",
            "Arrays.asList(matrix).subList(0, Math.min(matrix.length, maxLen))",
        ],
    );
}
#[test]
fn array_contents_disabled_does_not_allocate_limit() {
    let s = run(
        "package p; public class A { int[] ints; }",
        json!({"java.codeGeneration.toString.limitElements":2,"java.codeGeneration.toString.listArrayContents":false}),
        &["ints"],
    );
    contains(&s, &["return \"A [ints=\" + ints + \"]\";"]);
    assert!(!s.contains("maxLen"));
}
#[test]
fn unlimited_multidimensional_arrays_use_to_string() {
    let s = run(
        "package p; public class A { int[][] matrix; }",
        json!({}),
        &["matrix"],
    );
    contains(&s, &["Arrays.toString(matrix)"]);
    assert!(!s.contains("deepToString"));
}
#[test]
fn custom_template_object_and_member_variables() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.template":"${object.getClassName}|${object.superToString}|${object.hashCode}|${object.identityHashCode} [${member.name}:${member.value}; ${otherMembers}]"}),
        &["id", "label"],
    );
    contains(
        &s,
        &[
            "getClass().getName()",
            "super.toString()",
            "hashCode()",
            "System.identityHashCode(this)",
            "\" [id:\" + id + \"; label:\" + label + \"]\"",
        ],
    );
}
#[test]
fn format_preserves_percent_and_unknown_template_variables() {
    let s = run(
        SIMPLE,
        json!({"java.codeGeneration.toString.codeStyle":"STRING_FORMAT","java.codeGeneration.toString.template":"100% ${unknown} [${member.name}=${member.value}, ${otherMembers}]"}),
        &["id"],
    );
    contains(&s, &["String.format(\"100% ${unknown} [id=%s]\", id)"]);
}
#[test]
fn binding_keys_control_selection_order_and_duplicates() {
    let (mut ws, _, uri) = setup(SIMPLE, json!({}));
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, SIMPLE);
    let mut fields = chosen(&status, &["id", "label"])
        .as_array()
        .unwrap()
        .clone();
    fields.reverse();
    fields.push(fields[0].clone());
    for f in &mut fields {
        f["name"] = json!("bogus");
        f["isField"] = json!(false);
        f["isSelected"] = json!(false);
    }
    let s = generate(&mut ws, &uri, SIMPLE, json!(fields));
    contains(
        &s,
        &["return \"A [id=\" + id + \", label=\" + label + \"]\";"],
    );
    assert!(!s.contains("bogus"));
}
#[test]
fn empty_and_unknown_selection_still_generates_method() {
    let (mut ws, _, uri) = setup(SIMPLE, json!({}));
    open(&mut ws, &uri);
    let fields = json!([{"bindingKey":"not-a-binding","name":"id","type":"int","isField":true,"isSelected":true}]);
    let s = generate(&mut ws, &uri, SIMPLE, fields);
    contains(&s, &["return \"A []\";"]);
}
#[test]
fn discovery_transient_methods_and_shadowed_fields() {
    let source="package p; class Parent { int id; int hidden; private int secret; static int shared; public int count() {return 1;} } public class A extends Parent { int id; transient int hidden; static int skip; int first, second; public int count(){return 2;} private String local(){return null;} void nothing(){} int args(int x){return x;} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    let f = status["fields"].as_array().unwrap();
    assert_eq!(
        f.iter().filter(|v| v["name"] == "id").count(),
        2,
        "{status}"
    );
    assert_eq!(
        f.iter().filter(|v| v["name"] == "hidden").count(),
        1,
        "{status}"
    );
    for n in ["secret", "shared", "skip", "nothing", "args", "clone"] {
        assert!(!f.iter().any(|v| v["name"] == n), "{status}");
    }
    assert!(f
        .iter()
        .any(|v| v["name"] == "hidden" && v["isSelected"] == false));
    let method = f.iter().find(|v| v["name"] == "local").unwrap();
    assert_eq!(method["parameters"], json!([]));
    assert_eq!(method["isField"], false);
    let field = f.iter().find(|v| v["name"] == "first").unwrap();
    assert!(field.get("parameters").is_none());
    assert_eq!(field["type"], "int");
}
#[test]
fn records_discover_and_generate_components() {
    let source = "package p; public record A(int id, String label) {}";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    let fields = json!(chosen(&status, &["id", "label"])
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["isField"] == true)
        .cloned()
        .collect::<Vec<_>>());
    assert_eq!(fields.as_array().unwrap().len(), 2, "{status}");
    let s = generate(&mut ws, &uri, source, fields);
    contains(
        &s,
        &["return \"A [id=\" + id + \", label=\" + label + \"]\";"],
    );
}
#[test]
fn replacement_preserves_overloaded_method_and_disk_source() {
    let source="package p; public class A { int id; public String toString(){return \"OLD\";} public String toString(int n){return \"overload\";} }";
    let (mut ws, _, uri) = setup(source, json!({}));
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    assert_eq!(status["exists"], true);
    let s = generate(&mut ws, &uri, source, chosen(&status, &["id"]));
    contains(
        &s,
        &[
            "return \"A [id=\" + id + \"]\";",
            "public String toString(int n){return \"overload\";}",
        ],
    );
    assert!(!s.contains("OLD"));
    assert_eq!(
        std::fs::read_to_string(
            tower_lsp::lsp_types::Url::parse(&uri)
                .unwrap()
                .to_file_path()
                .unwrap()
        )
        .unwrap(),
        source
    );
}
#[test]
fn local_names_avoid_static_fields_and_nested_types() {
    let s=run("package p; import java.util.Set; public class A { static int maxLen; static int builder; class iterator {} Set<String> set; }",json!({"java.codeGeneration.toString.codeStyle":"STRING_BUILDER", "java.codeGeneration.toString.limitElements":3}), &["set"]);
    contains(
        &s,
        &[
            "final int maxLen2 = 3;",
            "StringBuilder builder2 = new StringBuilder();",
            "Iterator<?> iterator2 = collection.iterator()",
        ],
    );
}
#[test]
fn existing_collection_helper_is_preserved() {
    let s=run("package p; import java.util.*; public class A { Set<String> set; private String toString(Collection<?> c, int m) { return \"existing\"; } }",json!({"java.codeGeneration.toString.limitElements":3}), &["set"]);
    contains(&s, &["return \"existing\";", "toString(set, maxLen)"]);
    assert_eq!(compact(&s).matches("toString(Collection<?>").count(), 1);
}
#[test]
fn differing_collection_helper_overload_does_not_suppress_generation() {
    let s=run("package p; import java.util.*; public class A { Set<String> set; private String toString(Collection<?> c, long m) { return \"overload\"; } }",json!({"java.codeGeneration.toString.limitElements":3}), &["set"]);
    contains(
        &s,
        &[
            "return \"overload\";",
            "private String toString(Collection<?> collection, int maxLen)",
        ],
    );
}
#[test]
fn virtual_open_buffers() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:ToString.java",
        "inmemory:/p/A.java",
        "file:///tmp/nonexistent-tostring/A.java",
    ] {
        let mut ws = Workspace::new();
        ws.client().notify(
            "textDocument/didOpen",
            json!({"textDocument":{"uri":uri,"languageId":"java","version":1,"text":SIMPLE}}),
        );
        let status = discover(&mut ws, uri, SIMPLE);
        let s = generate(&mut ws, uri, SIMPLE, chosen(&status, &["id"]));
        contains(&s, &["return \"A [id=\" + id + \"]\";"]);
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
                .is_some_and(|s| s.starts_with("Generate toString()"))
        })
        .cloned()
        .collect()
}
#[test]
fn prompt_capability_and_kind_filter() {
    let (mut ws, _, uri) = setup(SIMPLE, json!({}));
    open(&mut ws, &uri);
    assert!(actions(&mut ws, &uri, SIMPLE, "").is_empty());
    let (mut ws, _, uri) = setup(SIMPLE, json!({}));
    ws.init_options["extendedClientCapabilities"]["generateToStringPromptSupport"] = json!(true);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, SIMPLE, "");
    assert_eq!(a.len(), 2, "{a:?}");
    for a in a {
        assert_eq!(
            a["command"]["command"],
            "java.action.generateToStringPrompt"
        );
        assert_eq!(a["command"]["arguments"][0], params(&uri, SIMPLE));
    }
    assert_eq!(
        actions(&mut ws, &uri, SIMPLE, "source.generate.toString").len(),
        1
    );
    assert!(actions(&mut ws, &uri, SIMPLE, "quickassist").is_empty());
}
#[test]
fn unsupported_literal_kind_returns_command() {
    let (mut ws, _, uri) = setup(SIMPLE, json!({}));
    ws.init_options["extendedClientCapabilities"]["generateToStringPromptSupport"] = json!(true);
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, SIMPLE, "");
    assert!(!a.is_empty());
    for a in a {
        assert_eq!(a["command"], "java.action.generateToStringPrompt");
        assert!(a["kind"].is_null());
    }
    assert!(actions(&mut ws, &uri, SIMPLE, "source.generate.toString").is_empty());
}
#[test]
fn direct_actions_are_eager_or_resolved_without_prompt_capability() {
    let source = "package p; public class A {}";
    for resolve in [false, true] {
        let (mut ws, _, uri) = setup(source, json!({}));
        if !resolve {
            ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(false);
        }
        open(&mut ws, &uri);
        let a = actions(&mut ws, &uri, source, "source.generate.toString");
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
        contains(&actual, &["return \"A []\";"]);
    }
}
fn override_template(root: &std::path::Path, key: &str, pattern: &str, extras: &str) {
    let xml=format!("<templates><template id=\"org.eclipse.jdt.ui.text.codetemplates.{key}\" name=\"{key}\" description=\"{key}\" context=\"{key}_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">{pattern}</template></templates>");
    std::fs::write(root.join(".settings/org.eclipse.jdt.ls.core.prefs"),format!("eclipse.preferences.version=1\n{extras}\norg.eclipse.jdt.ui.text.custom_code_templates={}\n",xml.replace('\\',"\\\\").replace('\n',"\\n"))).unwrap();
}
#[test]
fn override_comment_expands_compilation_unit_variables() {
    let source = "package p; public class A { int id; }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    override_template(&root,"overridecomment","/** ${file_name} ${package_name} ${enclosing_type} ${enclosing_method} ${project_name}\n * ${see_to_overridden}\n */","");
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&status, &["id"]));
    contains(
        &s,
        &[
            "A.java p p.A toString TestProject",
            "@see java.lang.Object#toString()",
        ],
    );
    assert!(!s.contains("${"));
}
#[test]
fn java23_uses_ordinary_override_template() {
    let source = "package p; public class A { int id; }";
    let (mut ws, root, uri) = setup(source, json!({"java.codeGeneration.generateComments":true}));
    let path = root.join(".settings/org.eclipse.jdt.core.prefs");
    let prefs = std::fs::read_to_string(&path)
        .unwrap()
        .replace("=21", "=23");
    std::fs::write(path, prefs).unwrap();
    override_template(
        &root,
        "markdownoverridecomment",
        "/// ${enclosing_type}\n/// ${see_to_overridden}",
        "org.eclipse.jdt.ui.usemarkdown=true",
    );
    let prefs_path = root.join(".settings/org.eclipse.jdt.ls.core.prefs");
    let prefs = std::fs::read_to_string(&prefs_path).unwrap().replace("</templates>", "<template id=\"org.eclipse.jdt.ui.text.codetemplates.overridecomment\" name=\"overridecomment\" description=\"overridecomment\" context=\"overridecomment_context\" enabled=\"true\" deleted=\"false\" autoinsert=\"true\">/** Standard ${enclosing_type} */</template></templates>");
    std::fs::write(prefs_path, prefs).unwrap();
    open(&mut ws, &uri);
    let status = discover(&mut ws, &uri, source);
    let s = generate(&mut ws, &uri, source, chosen(&status, &["id"]));
    contains(&s, &["/** Standard p.A */"]);
    assert!(!s.contains("///"));
}
