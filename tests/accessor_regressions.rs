//! Public accessor protocol cases beyond the eighteen upstream tests.
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
    let mut p = params(uri, source, token);
    p["kind"] = json!("BOTH");
    ws.request("java/resolveUnimplementedAccessors", p)
}
fn generate(ws: &mut Workspace, uri: &str, source: &str, token: &str, fields: Value) -> String {
    let edit = ws.request(
        "java/generateAccessors",
        json!({"context":params(uri,source,token),"accessors":fields}),
    );
    apply_edits(
        source,
        edit["changes"][uri].as_array().expect("workspace changes"),
    )
}
fn field(name: &str, stat: bool, get: bool, set: bool, ty: &str) -> Value {
    json!({"fieldName":name,"isStatic":stat,"generateGetter":get,"generateSetter":set,"typeName":ty})
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
fn boolean_static_final_and_acronym_names() {
    let source="package p; public class A { boolean ready, isActive; Boolean boxed; int xPos; String URL; static int count; static final int MAX_SIZE=1; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let fields = discover(&mut ws, &uri, source, "class A");
    assert_eq!(
        fields,
        json!([
            field("ready", false, true, true, "boolean"),
            field("isActive", false, true, true, "boolean"),
            field("boxed", false, true, true, "Boolean"),
            field("xPos", false, true, true, "int"),
            field("URL", false, true, true, "String"),
            field("count", true, true, true, "int"),
            field("MAX_SIZE", true, true, false, "int")
        ])
    );
    let actual = generate(&mut ws, &uri, source, "class A", fields);
    contains(
        &actual,
        &[
            "boolean isReady()",
            "boolean isActive()",
            "void setActive(boolean isActive)",
            "Boolean getBoxed()",
            "int getxPos()",
            "void setURL(String uRL)",
            "public static int getCount()",
            "A.count = count;",
            "int getMaxSize()",
        ],
    );
    assert!(!actual.contains("setMaxSize"));
}
#[test]
fn prefixes_suffixes_argument_preferences_and_this() {
    let source =
        "package p; public class A { int mValue_; static String sName_; boolean mIsReady_; }";
    let options = [
        ("org.eclipse.jdt.core.codeComplete.fieldPrefixes", "m"),
        ("org.eclipse.jdt.core.codeComplete.fieldSuffixes", "_"),
        ("org.eclipse.jdt.core.codeComplete.staticFieldPrefixes", "s"),
        ("org.eclipse.jdt.core.codeComplete.staticFieldSuffixes", "_"),
        ("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p"),
        ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
    ];
    let (mut ws, _, uri) = setup(
        source,
        "org.eclipse.jdt.ui.keywordthis=true\norg.eclipse.jdt.ui.gettersetter.use.is=false\n",
        &options,
    );
    open(&mut ws, &uri);
    let f = discover(&mut ws, &uri, source, "class A");
    let actual = generate(&mut ws, &uri, source, "class A", f);
    contains(
        &actual,
        &[
            "int getValue() { return this.mValue_; }",
            "void setValue(int pValueArg) { this.mValue_ = pValueArg; }",
            "String getName() { return sName_; }",
            "void setName(String pNameArg) { sName_ = pNameArg; }",
            "boolean getIsReady()",
            "void setIsReady(boolean pIsReadyArg)",
        ],
    );
}
#[test]
fn existing_methods_match_generic_parameters_but_ignore_return_type_and_inheritance() {
    let source="package p; class Parent { int getInherited(){return 0;} } public class A extends Parent { boolean active; java.util.List<String>[] values; int wrong, inherited; void getActive(){} void setActive(boolean a){} void setValues(java.util.List<Integer>... v){} void setWrong(String w){} }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    assert_eq!(
        discover(&mut ws, &uri, source, "class A"),
        json!([
            field("values", false, true, true, "List<String>[]"),
            field("wrong", false, true, true, "int"),
            field("inherited", false, true, true, "int")
        ])
    );
}
#[test]
fn selected_nested_local_and_anonymous_types() {
    let source="package p; public class A { int outer; class Inner { String inner; } void f(){ class Local { int local; } Object o=new Object(){boolean anon;}; } }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    for (token, name, ty) in [
        ("Inner", "inner", "String"),
        ("Local", "local", "int"),
        ("anon", "anon", "boolean"),
    ] {
        assert_eq!(
            discover(&mut ws, &uri, source, token),
            json!([field(name, false, true, true, ty)])
        );
    }
    let f = discover(&mut ws, &uri, source, "Inner");
    let actual = generate(&mut ws, &uri, source, "Inner", f);
    contains(
        &actual,
        &[
            "class Inner { String inner; public String getInner()",
            "this.inner = inner;",
        ],
    );
    assert!(!actual.contains("getOuter"));
}
#[test]
fn enum_fields_record_components_and_annotation_exclusion() {
    let source = "package p; public enum A { FIRST; int number; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    assert_eq!(
        discover(&mut ws, &uri, source, "enum A"),
        json!([field("number", false, true, true, "int")])
    );
    let annotation = "package p; public @interface A { int VALUE=1; }";
    ws.change(&uri, annotation);
    ws.diagnostics(&uri);
    ws.request("java/buildWorkspace", json!(false));
    assert_eq!(discover(&mut ws, &uri, annotation, "A"), json!([]));
}
#[test]
fn record_discovery_excludes_static_fields_and_declared_accessor() {
    let source="package p; public record A(String name, int age) { static int other; public String name(){ return name; } }";
    let (mut ws, _, uri) = setup(
        source,
        "",
        &[
            ("org.eclipse.jdt.core.compiler.source", "21"),
            ("org.eclipse.jdt.core.compiler.compliance", "21"),
            ("org.eclipse.jdt.core.compiler.codegen.targetPlatform", "21"),
        ],
    );
    open(&mut ws, &uri);
    assert_eq!(
        discover(&mut ws, &uri, source, "A"),
        json!([
            field("name", false, false, true, "String"),
            field("age", false, true, true, "int")
        ])
    );
}
#[test]
fn utf16_crlf_and_open_buffer_generation() {
    let disk = "package p; public class A {}";
    let source="package p;\r\npublic class A {\r\n\t// 😀 before field\r\n\tjava.util.Map<String, java.lang.Integer>[] values;\r\n}\r\n";
    let (mut ws, root, uri) = setup(disk, "", &[]);
    ws.open_with(&uri, source);
    ws.diagnostics(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let f = discover(&mut ws, &uri, source, "values");
    assert_eq!(
        f,
        json!([field(
            "values",
            false,
            true,
            true,
            "Map<String,java.lang.Integer>[]"
        )])
    );
    let actual = generate(&mut ws, &uri, source, "class A", f);
    contains(
        &actual,
        &[
            "java.util.Map<String, java.lang.Integer>[] getValues()",
            "void setValues(java.util.Map<String, java.lang.Integer>[] values)",
        ],
    );
    assert!(actual.contains("// 😀 before field\r\n"));
    assert!(!actual.replace("\r\n", "").contains('\n'));
    assert_eq!(
        std::fs::read_to_string(root.join("src/p/A.java")).unwrap(),
        disk
    );
}
#[test]
fn virtual_documents_support_discovery_and_generation() {
    if is_oracle() {
        return;
    } // JDT's ICompilationUnit requires a filesystem resource.
    let source = "public class A { boolean ready; }";
    let mut ws = Workspace::new();
    let uri = "untitled:Accessor.java";
    ws.open_with(uri, source);
    let f = discover(&mut ws, uri, source, "ready");
    assert_eq!(f, json!([field("ready", false, true, true, "boolean")]));
    let actual = generate(&mut ws, uri, source, "class A", f);
    contains(
        &actual,
        &["boolean isReady()", "void setReady(boolean ready)"],
    );
}
#[test]
fn quickassist_and_source_kind_filters() {
    let source = "package p; public class A { private int value; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.settings["java.quickfix.showAt"] = json!("problem");
    open(&mut ws, &uri);
    let on_modifier = actions(&mut ws, &uri, source, "private", "");
    assert!(on_modifier
        .iter()
        .any(|a| a["title"] == "Generate Getters" && a["kind"] == "source.generate.accessors"));
    assert!(!on_modifier
        .iter()
        .any(|a| a["title"] == "Generate Getter for 'value'"));
    let on_name = actions(&mut ws, &uri, source, "value", "");
    assert!(on_name
        .iter()
        .any(|a| a["title"] == "Generate Getter for 'value'" && a["kind"] == "quickassist"));
    for kind in ["quickassist", "source.generate.accessors"] {
        let a = actions(&mut ws, &uri, source, "value", kind);
        let getters: Vec<_> = a
            .iter()
            .filter(|a| {
                a["title"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("Generate Getter"))
            })
            .collect();
        assert_eq!(
            getters.len(),
            if kind == "quickassist" { 0 } else { 2 },
            "{a:?}"
        );
        assert!(getters.iter().all(|a| a["kind"] == kind));
    }
}
#[test]
fn advanced_prompt_falls_back_to_command_without_literal_kind_support() {
    let source = "package p; public class A { int one, two; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    ws.init_options["extendedClientCapabilities"]["advancedGenerateAccessorsSupport"] = json!(true);
    ws.capabilities["textDocument"]["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]
        ["valueSet"] = json!(["quickfix"]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, "one", "");
    let prompt = a
        .iter()
        .find(|a| a["title"] == "Generate Getters and Setters...")
        .expect("prompt command");
    assert_eq!(prompt["command"], "java.action.generateAccessorsPrompt");
    assert_eq!(prompt["arguments"][0]["kind"], 2);
    assert!(prompt["kind"].is_null());
}
#[test]
fn deferred_code_action_resolve_produces_selected_field_edit() {
    let source = "package p; public class A { int one, two; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let a = actions(&mut ws, &uri, source, "one", "");
    let getter = a
        .iter()
        .find(|a| a["title"] == "Generate Getter for 'one'")
        .expect("getter");
    assert!(getter["edit"].is_null());
    assert!(!getter["data"].is_null());
    let resolved = ws.request("codeAction/resolve", getter.clone());
    let actual = apply_edits(
        source,
        resolved["edit"]["changes"][&uri].as_array().unwrap(),
    );
    contains(&actual, &["int getOne() { return one; }"]);
    assert!(!actual.contains("getTwo"));
}
#[test]
fn custom_templates_expand_fields_methods_and_qualified_enclosing_type() {
    let source = "package p; public class A { class Inner { int value; } }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    templates(&root,&[("gettercomment","/** ${enclosing_type}.${enclosing_method} ${field} ${field_type} ${bare_field_name} ${file_name} ${package_name} */"),("getterbody","return ${field} + 1;"),("settercomment","/** ${enclosing_type}.${enclosing_method} ${field} ${param} */"),("setterbody","${field} = ${param} + 1;")],"org.eclipse.jdt.ui.keywordthis=true");
    open(&mut ws, &uri);
    let f = discover(&mut ws, &uri, source, "Inner");
    let actual = generate(&mut ws, &uri, source, "Inner", f);
    contains(
        &actual,
        &[
            "/** A.Inner.getValue value int value A.java p */",
            "return this.value + 1;",
            "/** A.Inner.setValue value value */",
            "this.value = value + 1;",
        ],
    );
}
#[test]
fn empty_and_missing_fields_return_no_edit() {
    let source = "package p; public class A { int value; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    for fields in [
        Value::Null,
        json!([]),
        json!([field("missing", false, true, true, "int")]),
    ] {
        assert!(ws
            .request(
                "java/generateAccessors",
                json!({"context":params(&uri,source,"class A"),"accessors":fields})
            )
            .is_null());
    }
}

#[test]
fn unicode_identifier_names_use_java_character_case_rules() {
    let source = "package p; public class A { int ßet, İtem, 𐐨name, $URL; }";
    let (mut ws, _, uri) = setup(source, "", &[]);
    open(&mut ws, &uri);
    let f = discover(&mut ws, &uri, source, "class A");
    let actual = generate(&mut ws, &uri, source, "class A", f);
    contains(
        &actual,
        &[
            "int getßet()",
            "int getİtem()",
            "int get𐐨name()",
            "void setİtem(int item)",
            "void set$URL(int $url)",
        ],
    );
}
#[test]
fn markdown_comment_preference_obeys_java_compliance() {
    for level in ["21", "23"] {
        let source = "package p; public class A { int value; }";
        let (mut ws, root, uri) = setup(
            source,
            "org.eclipse.jdt.ui.usemarkdown=true\n",
            &[
                ("org.eclipse.jdt.core.compiler.source", level),
                ("org.eclipse.jdt.core.compiler.compliance", level),
                (
                    "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
                    level,
                ),
            ],
        );
        ws.settings["java.codeGeneration.generateComments"] = json!(true);
        if level == "23" {
            templates(
                &root,
                &[
                    (
                        "markdowngettercomment",
                        "/// @return the ${bare_field_name}",
                    ),
                    (
                        "markdownsettercomment",
                        "/// @param ${param} the ${bare_field_name} to set",
                    ),
                ],
                "org.eclipse.jdt.ui.usemarkdown=true",
            );
        }
        open(&mut ws, &uri);
        let f = discover(&mut ws, &uri, source, "class A");
        let actual = generate(&mut ws, &uri, source, "class A", f);
        if level == "23" {
            contains(
                &actual,
                &["/// @return the value", "/// @param value the value to set"],
            );
            assert!(!actual.contains("/**"));
        } else {
            contains(
                &actual,
                &[
                    "/**",
                    "* @return the value",
                    "* @param value the value to set",
                ],
            );
            assert!(!actual.contains("///"));
        }
    }
}
#[test]
fn empty_template_bodies_and_comments_generate_empty_methods() {
    let source = "package p; public class A { int value; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    templates(
        &root,
        &[
            ("gettercomment", " "),
            ("settercomment", ""),
            ("getterbody", ""),
            ("setterbody", ""),
        ],
        "",
    );
    open(&mut ws, &uri);
    let f = discover(&mut ws, &uri, source, "class A");
    let actual = generate(&mut ws, &uri, source, "class A", f);
    contains(
        &actual,
        &[
            "public int getValue() {}",
            "public void setValue(int value) {}",
        ],
    );
    assert!(!actual.contains("/**"));
}

#[test]
fn template_dollar_escapes_aliases_and_unknown_variables() {
    let source = "package p; public class A { int value; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    ws.settings["java.codeGeneration.generateComments"] = json!(true);
    templates(
        &root,
        &[
            (
                "gettercomment",
                "/** $$ $${field} ${dollar} ${alias:field} ${unknown} */",
            ),
            ("settercomment", ""),
        ],
        "",
    );
    open(&mut ws, &uri);
    let f = discover(&mut ws, &uri, source, "class A");
    let actual = generate(&mut ws, &uri, source, "class A", f);
    contains(&actual, &["/** $ ${field} $ value unknown */"]);
}
#[test]
fn malformed_template_returns_null_and_numeric_kinds_round_trip() {
    let source = "package p; public class A { int value; }";
    let (mut ws, root, uri) = setup(source, "", &[]);
    templates(&root, &[("getterbody", "return ${field;")], "");
    open(&mut ws, &uri);
    let mut p = params(&uri, source, "class A");
    p["kind"] = json!(0);
    let f = ws.request("java/resolveUnimplementedAccessors", p);
    assert_eq!(f, json!([field("value", false, true, false, "int")]));
    assert!(ws
        .request(
            "java/generateAccessors",
            json!({"context":params(&uri,source,"class A"),"accessors":f})
        )
        .is_null());
}
