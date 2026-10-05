//! Annotation-aware ImportRewrite type nodes, compared with the Java server.
mod common;
use common::jdtls::test_default_options;
use common::jdtls::{apply_edits, is_oracle, Workspace};
use common::quickfix::get_range;
use common::quickfix::QuickFixTest;
use serde_json::json;

fn constructor(source: &str) -> String {
    generate(source, "constructor", &[])
}
fn generate(source: &str, kind: &str, files: &[(&str, &str, &str)]) -> String {
    let mut ws = Workspace::new();
    ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(true);
    ws.init_options["extendedClientCapabilities"]["generateDelegateMethodsPromptSupport"] =
        json!(true);
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    let root = ws.new_empty_project(&options);
    for (package, file, text) in files {
        ws.create_cu(&root, "src", package, file, text);
    }
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let context = json!({"textDocument":{"uri":uri},"range":get_range(source,"class A"),"context":{"diagnostics":[]}});
    let edit = if kind == "constructor" {
        let status = ws.request("java/checkConstructorsStatus", context.clone());
        let fields: Vec<_> = status["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["name"] == "marked")
            .cloned()
            .collect();
        assert_eq!(fields.len(), 1, "{status}");
        ws.request(
            "java/generateConstructors",
            json!({"context":context,"constructors":status["constructors"],"fields":fields}),
        )
    } else if kind == "override" {
        let status = ws.request("java/listOverridableMethods", context.clone());
        let methods: Vec<_> = status["methods"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["name"] == "read")
            .cloned()
            .collect();
        assert_eq!(methods.len(), 1, "{status}");
        ws.request(
            "java/addOverridableMethods",
            json!({"context":context,"overridableMethods":methods}),
        )
    } else {
        let status = ws.request("java/checkDelegateMethodsStatus", context.clone());
        let field = status["delegateFields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["field"]["name"] == "b")
            .expect("delegate field");
        let entries: Vec<_> = field["delegateMethods"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["name"] == "read")
            .map(|m| json!({"field":field["field"],"delegateMethod":m}))
            .collect();
        assert_eq!(entries.len(), 1, "{status}");
        ws.request(
            "java/generateDelegateMethods",
            json!({"context":context,"delegateEntries":entries}),
        )
    };
    apply_edits(
        source,
        edit["changes"][&uri]
            .as_array()
            .expect("constructor changes"),
    )
}

const FLAG: &str =
    "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}";
fn field_type(typ: &str, declarations: &str, expected: &str) {
    let source =
        format!("package p; public class A {{String plain; {typ} marked;}} {declarations}");
    let result = constructor(&source);
    assert!(
        compact(&result).contains(&compact(&format!("public A({expected} marked)"))),
        "{result}"
    );
}

#[test]
fn primitive_annotation() {
    field_type("@Flag int", FLAG, "@Flag int");
}
#[test]
fn generic_argument_annotation() {
    field_type("java.util.List<@Flag String>", FLAG, "List<@Flag String>");
}
#[test]
fn wildcard_and_bound_annotations() {
    field_type(
        "java.util.List<@Flag ? extends @Flag Number>",
        FLAG,
        "List<@Flag ? extends @Flag Number>",
    );
}
#[test]
fn lower_wildcard_bound_annotation() {
    field_type(
        "java.util.List<? super @Flag String>",
        FLAG,
        "List<? super @Flag String>",
    );
}
#[test]
fn array_element_and_dimension_annotations() {
    field_type(
        "@Flag String @Flag [] @Flag []",
        FLAG,
        "@Flag String @Flag [] @Flag []",
    );
}
#[test]
fn differently_annotated_variants_keep_distinct_values() {
    field_type("java.util.Map<@Flag(1) String, @Flag(2) String>", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value();}", "Map<@Flag(1) String, @Flag(2) String>");
}
#[test]
fn annotation_defaults_are_not_materialized() {
    field_type("@Flag String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value() default 5;}", "@Flag String");
}
#[test]
fn singleton_annotation_array_is_flattened() {
    field_type("@Flag({1}) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int[] value();}", "@Flag(1) String");
}
#[test]
fn empty_annotation_array_remains_empty() {
    field_type("@Flag({}) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int[] value();}", "@Flag({}) String");
}
#[test]
fn multiple_annotation_array_values() {
    field_type("@Flag({1,2}) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int[] value();}", "@Flag({1,2}) String");
}
#[test]
fn annotation_constant_expressions_are_resolved() {
    field_type("@Flag(1+2) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value();}", "@Flag(3) String");
}
#[test]
fn annotation_boolean_character_and_string_values() {
    field_type(r#"@Flag(b=true,c='\n',s="a\"b\\c") String"#, "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {boolean b(); char c(); String s();}", r#"@Flag(b=true,c='\n',s="a\"b\\c") String"#);
}
#[test]
fn annotation_numeric_values_use_binding_text() {
    field_type("@Flag(l=2L,f=1.5F,d=3.25) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {long l();float f();double d();}", "@Flag(l=2,f=1.5,d=3.25) String");
}
#[test]
fn nested_annotation_values() {
    field_type("@Flag(@Inner(4)) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {Inner value();} @interface Inner {int value();}", "@Flag(@Inner(4)) String");
}
#[test]
fn annotation_enum_and_class_values_add_imports() {
    field_type("@Flag(e=java.lang.annotation.ElementType.METHOD,t=java.util.List.class) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {java.lang.annotation.ElementType e();Class<?> t();}", "@Flag(e=ElementType.METHOD,t=List.class) String");
}
#[test]
fn annotated_type_variable() {
    let source = format!("package p; public class A<T> {{T plain; @Flag T marked;}} {FLAG}");
    let result = constructor(&source);
    assert!(
        compact(&result).contains(&compact("public A(@Flag T marked)")),
        "{result}"
    );
}
#[test]
fn annotated_generic_owner_is_preserved() {
    field_type(
        "Outer<@Flag String>.@Flag Inner<@Flag Integer>",
        &format!("{FLAG} class Outer<T> {{class Inner<U> {{}}}}"),
        "Outer<@Flag String>.@Flag Inner<@Flag Integer>",
    );
}
#[test]
fn annotation_import_conflict_uses_qualified_name() {
    let source = "package p; public class A { @q.Flag String marked; } class Flag {}";
    let result = generate(source, "constructor", &[("q","Flag.java","package q; @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) public @interface Flag {}")]);
    assert!(
        compact(&result).contains(&compact("public A(@q.Flag String marked)")),
        "{result}"
    );
}
#[test]
fn annotated_type_import_conflict_places_annotation_after_qualifier() {
    let source = "package p; public class A { java.util.@Flag List<String> marked; } class List {} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}";
    let result = constructor(source);
    assert!(
        compact(&result).contains(&compact("public A(java.util.@Flag List<String> marked)")),
        "{result}"
    );
}
#[test]
fn overridden_return_and_parameter_types_keep_annotations() {
    let source = format!("package p; public class A implements I {{}} interface I {{ @Flag String read(@Flag String input); }} {FLAG}");
    let result = generate(&source, "override", &[]);
    assert!(
        compact(&result).contains(&compact("public @Flag String read(@Flag String input)")),
        "{result}"
    );
}
#[test]
fn delegated_return_and_parameter_types_keep_annotations() {
    let source = format!("package p; public class A {{ B b; }} class B {{ public @Flag String read(@Flag String input) {{return input;}} }} {FLAG}");
    let result = generate(&source, "delegate", &[]);
    assert!(
        compact(&result).contains(&compact("public @Flag String read(@Flag String input)")),
        "{result}"
    );
}
#[test]
fn overridden_varargs_keep_element_and_dimension_annotations() {
    let source = format!("package p; public class A implements I {{}} interface I {{ void read(@Flag String @Flag ... input); }} {FLAG}");
    let result = generate(&source, "override", &[]);
    assert!(
        compact(&result).contains(&compact("public void read(@Flag String @Flag ... input)")),
        "{result}"
    );
}
#[test]
fn delegated_multidimensional_varargs_keep_annotations_in_order() {
    let source = "package p; public class A { B b; } class B { public void read(@Flag(0) String @Flag(1) [] @Flag(2) ... input) {} } @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value();}";
    let result = generate(source, "delegate", &[]);
    assert!(
        compact(&result).contains(&compact(
            "public void read(@Flag(0) String @Flag(1) [] @Flag(2) ... input)"
        )),
        "{result}"
    );
}
#[test]
fn overridden_type_bounds_and_exceptions_keep_annotations() {
    let source = format!("package p; public class A implements I {{}} interface I {{ <T extends @Flag Number> T read() throws @Flag Exception; }} {FLAG}");
    let result = generate(&source, "override", &[]);
    assert!(
        compact(&result).contains(&compact(
            "public <T extends @Flag Number> T read() throws @Flag Exception"
        )),
        "{result}"
    );
}

#[test]
fn delegated_captured_return_argument_retains_its_wildcard_bound() {
    let source = "package p; public class A { B<? extends Number> b; } class B<T> { public java.util.List<T> read() {return null;} }";
    let result = generate(source, "delegate", &[]);
    assert!(
        compact(&result).contains(&compact("public List<? extends Number> read()")),
        "{result}"
    );
}

#[test]
fn constructor_super_varargs_keep_dimension_annotation_order() {
    let source = "package p; public class A extends B {String marked;} class B { public B(@Flag(0) String @Flag(1) [] @Flag(2) ... input) {} } @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value();}";
    let result = constructor(source);
    assert!(
        compact(&result).contains(&compact(
            "public A(@Flag(0) String @Flag(1) [] @Flag(2) ... input, String marked)"
        )),
        "{result}"
    );
}

#[test]
fn annotation_class_array_literal_adds_element_import() {
    field_type("@Flag(java.util.List[].class) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {Class<?> value();}", "@Flag(List[].class) String");
}

#[test]
fn qualified_annotation_and_value_types_are_imported() {
    let source = "package p; public class A { @q.Flag(q.Mode.ON) String marked; }";
    let result = generate(source, "constructor", &[
        ("q","Flag.java","package q; @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) public @interface Flag {Mode value();}"),
        ("q","Mode.java","package q; public enum Mode {ON,OFF}"),
    ]);
    for expected in [
        "import q.Flag;",
        "import q.Mode;",
        "public A(@Flag(Mode.ON) String marked)",
    ] {
        assert!(compact(&result).contains(&compact(expected)), "{result}");
    }
}

#[test]
fn equally_named_annotations_from_distinct_packages_do_not_merge() {
    let source = "package p; public class A {java.util.Map<@q.Flag String,@r.Flag String> marked;}";
    let result = generate(source, "constructor", &[
        ("q", "Flag.java", "package q; @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) public @interface Flag {}"),
        ("r", "Flag.java", "package r; @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) public @interface Flag {}"),
    ]);
    assert!(
        compact(&result).contains(&compact(
            "public A(Map<@Flag String,@r.Flag String> marked)"
        )),
        "{result}"
    );
    assert!(result.contains("import q.Flag;"), "{result}");
    assert!(!result.contains("import r.Flag;"), "{result}");
}

#[test]
fn repeated_type_annotations_preserve_source_order() {
    field_type("@Flag(1) @Flag(2) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @java.lang.annotation.Repeatable(Flags.class) @interface Flag {int value();} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flags {Flag[] value();}", "@Flag(1) @Flag(2) String");
}

#[test]
fn annotated_unsaved_buffer_overrides_disk_source() {
    let mut ws = Workspace::new();
    let root = ws.new_empty_project(&test_default_options());
    let disk = "package p; public class A { String marked; }";
    let uri = ws.create_cu(&root, "src", "p", "A.java", disk);
    let source = format!("package p; public class A {{ @Flag String marked; }} {FLAG}");
    ws.open(&uri);
    ws.change(&uri, &source);
    let context = json!({"textDocument":{"uri":uri},"range":get_range(&source,"class A"),"context":{"diagnostics":[]}});
    let status = ws.request("java/checkConstructorsStatus", context.clone());
    let edit = ws.request(
        "java/generateConstructors",
        json!({"context":context,"constructors":status["constructors"],"fields":status["fields"]}),
    );
    let result = apply_edits(&source, edit["changes"][&uri].as_array().unwrap());
    assert!(
        compact(&result).contains(&compact("public A(@Flag String marked)")),
        "{result}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/p/A.java")).unwrap(),
        disk
    );
}

#[test]
fn annotated_constructor_types_work_in_virtual_documents() {
    if is_oracle() {
        return;
    } // jdt.ls requires a resource-backed ICompilationUnit.
    let source = format!(
        "public class A {{String plain; java.util.List<@Flag String> @Flag [] marked;}} {FLAG}"
    );
    for scheme in ["untitled", "inmemory", "file"] {
        let mut ws = Workspace::new();
        let path = ws.dir.join("A.java");
        let uri = match scheme {
            "untitled" => "untitled:A.java".to_owned(),
            "inmemory" => "inmemory:///A.java".to_owned(),
            _ => tower_lsp::lsp_types::Url::from_file_path(&path)
                .unwrap()
                .to_string(),
        };
        ws.open_with(&uri, &source);
        let context = json!({"textDocument":{"uri":uri},"range":get_range(&source,"class A"),"context":{"diagnostics":[]}});
        let status = ws.request("java/checkConstructorsStatus", context.clone());
        let fields: Vec<_> = status["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["name"] == "marked")
            .cloned()
            .collect();
        let edit = ws.request(
            "java/generateConstructors",
            json!({"context":context,"constructors":status["constructors"],"fields":fields}),
        );
        let result = apply_edits(&source, edit["changes"][&uri].as_array().unwrap());
        assert!(
            compact(&result).contains(&compact("public A(List<@Flag String> @Flag [] marked)")),
            "{scheme}: {result}"
        );
        assert!(result.contains("import java.util.List;"), "{result}");
        assert!(!path.exists());
    }
}
fn correction(source: &str) -> String {
    correction_title(source, "Create local variable using expression")
}
fn correction_title(source: &str, title: &str) -> String {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    let root = t.ws.new_empty_project(&options);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    let actions = t.evaluate_code_actions(&uri);
    let action = actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("{actions:#?}"));
    t.evaluate_code_action_command(action)
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
#[test]
fn annotated_cast_keeps_annotations_after_an_unannotated_binding() {
    let source="package p; public class A {void run(String input){((@Flag String)input);}} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}";
    let result = correction(source);
    assert!(
        compact(&result).contains(&compact("@Flag String string = (@Flag String)input;")),
        "{result}"
    );
}

#[test]
fn constructor_preserves_annotated_variant_of_previously_seen_type() {
    let source="package p; public class A {String plain; @Flag String marked;} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {}";
    let result = constructor(source);
    assert!(
        compact(&result).contains(&compact("public A(@Flag String marked)")),
        "{result}"
    );
}

#[test]
fn unimplemented_method_correction_uses_annotated_type_imports() {
    let source = format!("package p; public class A implements I {{}} interface I {{ java.util.List<@Flag String> read(@Flag String input); }} {FLAG}");
    let result = correction_title(&source, "Add unimplemented methods");
    assert!(
        compact(&result).contains(&compact(
            "public List<@Flag String> read(@Flag String input)"
        )),
        "{result}"
    );
    assert!(result.contains("import java.util.List;"), "{result}");
}

#[test]
fn negative_annotation_number_is_preserved() {
    field_type("@Flag(-3) String", "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {int value();}", "@Flag(-3) String");
}

#[test]
fn char_and_string_annotation_escapes_are_preserved() {
    field_type(r#"@Flag(c='\'',s="\t\b\r\f\n\\\"'é😀") String"#, "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {char c();String s();}", r#"@Flag(c='\'',s="\t\b\r\f\n\\\"'é😀") String"#);
}
