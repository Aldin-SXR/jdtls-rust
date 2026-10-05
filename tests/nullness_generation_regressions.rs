//! Nullness import filtering and inherited annotations, checked against JDT LS.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::get_range;
use serde_json::json;

const TYPE_USE: &str = "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface NonNull {} @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Nullable {}";
const DEFAULT: &str = "@java.lang.annotation.Target({java.lang.annotation.ElementType.TYPE,java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PACKAGE}) @interface Default {boolean value() default true;}";
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn run(source: &str, options: &[(&str, &str)]) -> String {
    run_kind(source, options, "override", &[])
}
fn run_kind(
    source: &str,
    options: &[(&str, &str)],
    kind: &str,
    files: &[(&str, &str, &str)],
) -> String {
    let mut ws = Workspace::new();
    if kind == "quickfix" {
        ws.capabilities = common::quickfix::quickfix_client_capabilities();
    }
    // Interactive mode preserves explicit project compiler options. The
    // default disabled mode resets null analysis during project import.
    ws.settings["java"]["compile"]["nullAnalysis"]["mode"] = json!("interactive");
    ws.init_options["extendedClientCapabilities"]["overrideMethodsPromptSupport"] = json!(true);
    let mut opts = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        opts.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "21".into());
    }
    for (key, value) in [
        (
            "org.eclipse.jdt.core.compiler.annotation.nullanalysis",
            "enabled",
        ),
        (
            "org.eclipse.jdt.core.compiler.annotation.nonnull",
            "p.NonNull",
        ),
        (
            "org.eclipse.jdt.core.compiler.annotation.nullable",
            "p.Nullable",
        ),
        (
            "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault",
            "p.Default",
        ),
    ]
    .into_iter()
    .chain(options.iter().copied())
    {
        opts.insert(key.into(), value.into());
    }
    let root = ws.new_empty_project(&opts);
    for (package, file, text) in files {
        ws.create_cu(&root, "src", package, file, text);
    }
    let uri = ws.create_cu(&root, "src", "p", "A.java", source);
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    let context = json!({"textDocument":{"uri":uri},"range":get_range(source,"class A"),"context":{"diagnostics":[]}});
    let edit = if kind == "quickfix" {
        let diagnostics = ws.diagnostics(&uri);
        let mut params = context.clone();
        params["context"] = json!({"diagnostics":diagnostics,"only":["quickfix"]});
        let actions = ws.request("textDocument/codeAction", params);
        actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == "Add unimplemented methods")
            .unwrap_or_else(|| panic!("{actions}"))
            .get("edit")
            .unwrap()
            .clone()
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
    } else if kind == "constructor" {
        let status = ws.request("java/checkConstructorsStatus", context.clone());
        ws.request("java/generateConstructors",json!({"context":context,"constructors":status["constructors"],"fields":status["fields"]}))
    } else {
        let status = ws.request("java/checkDelegateMethodsStatus", context.clone());
        let field = status["delegateFields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["field"]["name"] == "b")
            .unwrap();
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
            .or_else(|| {
                edit["documentChanges"]
                    .as_array()
                    .and_then(|cs| cs.iter().find(|c| c["textDocument"]["uri"] == uri))
                    .and_then(|c| c["edits"].as_array())
            })
            .unwrap_or_else(|| panic!("{edit}")),
    )
}
#[test]
fn default_nonnull_removes_redundant_return_and_parameter_annotations() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact("public String read(String input)")),
        "{result}"
    );
}
#[test]
fn default_nonnull_keeps_nullable_annotations() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @Nullable String read(@Nullable String input); }} {TYPE_USE} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact(
            "public @Nullable String read(@Nullable String input)"
        )),
        "{result}"
    );
}

const DECLARED: &str = "@java.lang.annotation.Target({java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PARAMETER}) @interface NonNull {} @java.lang.annotation.Target({java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PARAMETER}) @interface Nullable {}";
#[test]
fn declaration_nullness_annotations_are_inherited() {
    let source=format!("package p; public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {DECLARED} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact(
            "@NonNull public String read(@NonNull String input)"
        )),
        "{result}"
    );
}
#[test]
fn redundant_declaration_nonnull_is_not_inherited_under_default() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {DECLARED} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact("public String read(String input)")),
        "{result}"
    );
}
#[test]
fn nullable_declaration_annotations_are_inherited_under_nonnull_default() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @Nullable String read(@Nullable String input); }} {DECLARED} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact(
            "@Nullable public String read(@Nullable String input)"
        )),
        "{result}"
    );
}
#[test]
fn source_modifier_annotation_order_is_preserved() {
    let source=format!("package p; public class A extends B {{}} class B {{ public @NonNull String read(@NonNull String input) {{return input;}} }} {DECLARED} {DEFAULT}");
    let result = run(&source, &[]);
    assert!(
        compact(&result).contains(&compact(
            "public @NonNull String read(@NonNull String input)"
        )),
        "{result}"
    );
}

const LOCATIONS: &str = "enum Location {PARAMETER, RETURN_TYPE, FIELD, TYPE_PARAMETER, TYPE_BOUND, TYPE_ARGUMENT, ARRAY_CONTENTS} @java.lang.annotation.Target({java.lang.annotation.ElementType.TYPE,java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PACKAGE}) @interface Default {Location[] value() default {Location.PARAMETER,Location.RETURN_TYPE,Location.FIELD};}";
fn contains(result: &str, expected: &str) {
    assert!(
        compact(result).contains(&compact(expected)),
        "missing {expected}: {result}"
    );
}
#[test]
fn explicit_false_default_cancels_the_enclosing_default() {
    let source=format!("package p; @Default class Outer {{ @Default(false) public class A implements I {{}} }} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run(&source, &[]),
        "public @NonNull String read(@NonNull String input)",
    );
}
#[test]
fn explicit_empty_enum_default_cancels_the_enclosing_default() {
    let source=format!("package p; @Default class Outer {{ @Default({{}}) public class A implements I {{}} }} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run(&source, &[]),
        "public @NonNull String read(@NonNull String input)",
    );
}
#[test]
fn enclosing_type_default_applies_to_nested_type() {
    let source=format!("package p; @Default class Outer {{ public class A implements I {{}} }} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(&run(&source, &[]), "public String read(String input)");
}
#[test]
fn enum_default_filters_only_selected_locations() {
    let source=format!("package p; @Default(Location.PARAMETER) public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run(&source, &[]),
        "public @NonNull String read(String input)",
    );
}
#[test]
fn enum_defaults_are_resolved_when_annotation_has_no_explicit_value() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {LOCATIONS}");
    contains(&run(&source, &[]), "public String read(String input)");
}
#[test]
fn marker_default_without_members_covers_return_and_parameters() {
    let default =
        "@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE) @interface Default {}";
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {default}");
    contains(&run(&source, &[]), "public String read(String input)");
}
#[test]
fn type_qualifier_default_maps_element_types_to_locations() {
    let default="@java.lang.annotation.Target(java.lang.annotation.ElementType.ANNOTATION_TYPE) @interface TypeQualifierDefault {java.lang.annotation.ElementType[] value();} @TypeQualifierDefault(java.lang.annotation.ElementType.PARAMETER) @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE) @interface Default {}";
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {default}");
    contains(
        &run(&source, &[]),
        "public @NonNull String read(String input)",
    );
}
#[test]
fn secondary_default_annotation_name_is_recognized() {
    let default="@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE) @interface OtherDefault {}";
    let source=format!("package p; @OtherDefault public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT} {default}");
    contains(
        &run(
            &source,
            &[(
                "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault.secondary",
                " p.OtherDefault, , p.Unused ",
            )],
        ),
        "public String read(String input)",
    );
}
#[test]
fn nonnull_type_variable_uses_are_not_filtered_under_default() {
    let source=format!("package p; @Default public class A<T> implements I<T> {{}} interface I<T> {{ @NonNull T read(@NonNull T input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run(&source, &[]),
        "public @NonNull T read(@NonNull T input)",
    );
}
#[test]
fn generic_arguments_follow_type_argument_default() {
    let source=format!("package p; @Default(Location.TYPE_ARGUMENT) public class A implements I {{}} interface I {{ java.util.List<@NonNull String> read(java.util.List<@Nullable String> input); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run(&source, &[]),
        "public List<String> read(List<@Nullable String> input)",
    );
}
#[test]
fn arrays_follow_array_contents_and_parameter_defaults() {
    let source=format!("package p; @Default({{Location.ARRAY_CONTENTS,Location.PARAMETER}}) public class A implements I {{}} interface I {{ @NonNull String @NonNull [] @NonNull [] read(@NonNull String @NonNull [] @NonNull [] input); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run(&source, &[]),
        "public String @NonNull [][] read(String[][] input)",
    );
}
#[test]
fn exception_types_filter_both_nullness_annotations() {
    let source=format!("package p; public class A implements I {{}} interface I {{ void read() throws @NonNull Exception, @Nullable RuntimeException; }} {TYPE_USE} {DEFAULT}");
    contains(
        &run(&source, &[]),
        "public void read() throws Exception, RuntimeException",
    );
}
#[test]
fn nullanalysis_disabled_retains_type_annotations_under_default() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run(
            &source,
            &[(
                "org.eclipse.jdt.core.compiler.annotation.nullanalysis",
                "disabled",
            )],
        ),
        "public @NonNull String read(@NonNull String input)",
    );
}
#[test]
fn inherit_null_annotations_option_avoids_copying_declaration_annotations() {
    let source=format!("package p; public class A implements I {{}} interface I {{ @Nullable String read(@NonNull String input); }} {DECLARED} {DEFAULT}");
    contains(
        &run(
            &source,
            &[(
                "org.eclipse.jdt.core.compiler.annotation.inheritNullAnnotations",
                "enabled",
            )],
        ),
        "public String read(String input)",
    );
}
#[test]
fn unrelated_declaration_annotations_are_not_inherited() {
    let extra="@java.lang.annotation.Target({java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PARAMETER}) @interface Flag {}";
    let source=format!("package p; public class A implements I {{}} interface I {{ @Flag @NonNull String read(@Flag @Nullable String input); }} {DECLARED} {DEFAULT} {extra}");
    contains(
        &run(&source, &[]),
        "@NonNull public String read(@Nullable String input)",
    );
}
#[test]
fn delegate_filters_type_annotations_using_target_default() {
    let source=format!("package p; @Default public class A {{ B b; }} class B {{ public @NonNull String read(@NonNull String input) {{return input;}} }} {TYPE_USE} {DEFAULT}");
    contains(
        &run_kind(&source, &[], "delegate", &[]),
        "public String read(String input)",
    );
}
#[test]
fn delegate_copies_declaration_parameter_annotations_without_default_filter() {
    let source=format!("package p; @Default public class A {{ B b; }} class B {{ public @NonNull String read(@NonNull String input) {{return input;}} }} {DECLARED} {DEFAULT}");
    contains(
        &run_kind(&source, &[], "delegate", &[]),
        "public String read(@NonNull String input)",
    );
}
#[test]
fn constructor_field_parameter_filters_redundant_type_annotation() {
    let source = format!(
        "package p; @Default public class A {{ @NonNull String marked; }} {TYPE_USE} {DEFAULT}"
    );
    contains(
        &run_kind(&source, &[], "constructor", &[]),
        "public A(String marked)",
    );
}
#[test]
fn constructor_super_parameters_copy_declaration_nullness_annotations() {
    let source=format!("package p; @Default public class A extends B {{}} class B {{ B(@NonNull String input) {{}} }} {DECLARED} {DEFAULT}");
    contains(
        &run_kind(&source, &[], "constructor", &[]),
        "public A(@NonNull String input)",
    );
}
#[test]
fn annotated_varargs_dimensions_bypass_redundant_type_filter() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ void read(@NonNull String @NonNull ... input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run(&source, &[]),
        "public void read(@NonNull String @NonNull ... input)",
    );
}
#[test]
fn package_default_is_used_when_type_has_no_default() {
    let source=format!("package p; public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run_kind(
            &source,
            &[],
            "override",
            &[("p", "package-info.java", "@Default package p;")],
        ),
        "public String read(String input)",
    );
}
#[test]
fn type_false_default_overrides_package_default() {
    let source=format!("package p; @Default(false) public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run_kind(
            &source,
            &[],
            "override",
            &[("p", "package-info.java", "@Default package p;")],
        ),
        "public @NonNull String read(@NonNull String input)",
    );
}

#[test]
fn package_false_default_keeps_explicit_nonnull_annotations() {
    let source=format!("package p; public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run_kind(
            &source,
            &[],
            "override",
            &[("p", "package-info.java", "@Default(false) package p;")],
        ),
        "public @NonNull String read(@NonNull String input)",
    );
}
#[test]
fn wildcard_annotations_are_retained_while_bound_default_is_filtered() {
    let source=format!("package p; @Default({{Location.TYPE_ARGUMENT,Location.TYPE_BOUND}}) public class A implements I {{}} interface I {{ java.util.List<@NonNull ? extends @NonNull Number> read(); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run(&source, &[]),
        "public List<@NonNull ? extends Number> read()",
    );
}
#[test]
fn enclosing_method_default_applies_to_local_type() {
    let source=format!("package p; class Outer {{ @Default void run() {{ class A implements I {{}} }} }} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(&run(&source, &[]), "public String read(String input)");
}
#[test]
fn other_location_filters_annotations_on_qualifying_owner_type() {
    let flag="@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Flag {} class Outer<T> { class Inner<U> {} }";
    let source=format!("package p; public class A implements I {{}} interface I {{ @Flag Outer<String>.Inner<Integer> read(); }} {TYPE_USE} {DEFAULT} {flag}");
    contains(
        &run(&source, &[]),
        "public Outer<String>.Inner<Integer> read()",
    );
}
#[test]
fn external_source_modifier_order_is_preserved() {
    let source = format!("package p; public class A extends B {{}} {DECLARED} {DEFAULT}");
    contains(&run_kind(&source,&[],"override",&[("p","B.java","package p; public class B { public @NonNull String read(@Nullable String input) {return \"x\";} }")]),"public @NonNull String read(@Nullable String input)");
}

#[test]
fn package_enum_default_filters_only_parameter_locations() {
    let source=format!("package p; public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {LOCATIONS}");
    contains(
        &run_kind(
            &source,
            &[],
            "override",
            &[(
                "p",
                "package-info.java",
                "@Default(Location.PARAMETER) package p;",
            )],
        ),
        "public @NonNull String read(String input)",
    );
}
#[test]
fn virtual_documents_apply_configured_nullness_defaults() {
    if is_oracle() {
        return;
    } // The Java server needs an ICompilationUnit resource.
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    for scheme in ["untitled", "inmemory", "file"] {
        let mut ws = Workspace::new();
        ws.init_options["compilerOptions"] = json!({
            "org.eclipse.jdt.core.compiler.annotation.nullanalysis":"enabled",
            "org.eclipse.jdt.core.compiler.annotation.nonnull":"p.NonNull",
            "org.eclipse.jdt.core.compiler.annotation.nullable":"p.Nullable",
            "org.eclipse.jdt.core.compiler.annotation.nonnullbydefault":"p.Default",
        });
        let path = ws.dir.join("A.java");
        let uri = match scheme {
            "untitled" => "untitled:A.java".into(),
            "inmemory" => "inmemory:///A.java".into(),
            _ => tower_lsp::lsp_types::Url::from_file_path(&path)
                .unwrap()
                .to_string(),
        };
        ws.open_with(&uri, &source);
        let context = json!({"textDocument":{"uri":uri},"range":get_range(&source,"class A"),"context":{"diagnostics":[]}});
        let status = ws.request("java/listOverridableMethods", context.clone());
        let methods: Vec<_> = status["methods"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["name"] == "read")
            .cloned()
            .collect();
        let edit = ws.request(
            "java/addOverridableMethods",
            json!({"context":context,"overridableMethods":methods}),
        );
        let result = apply_edits(&source, edit["changes"][&uri].as_array().unwrap());
        contains(&result, "public String read(String input)");
        assert!(!path.exists());
    }
}

#[test]
fn secondary_nullness_declaration_annotations_are_not_copied() {
    let extra="@java.lang.annotation.Target({java.lang.annotation.ElementType.METHOD,java.lang.annotation.ElementType.PARAMETER}) @interface OtherNonNull {}";
    let source=format!("package p; public class A implements I {{}} interface I {{ @OtherNonNull String read(@OtherNonNull String input); }} {DECLARED} {DEFAULT} {extra}");
    contains(
        &run(
            &source,
            &[(
                "org.eclipse.jdt.core.compiler.annotation.nonnull.secondary",
                "p.OtherNonNull",
            )],
        ),
        "public String read(String input)",
    );
}
#[test]
fn source_modifier_order_survives_skipped_unrelated_annotations() {
    let extra =
        "@java.lang.annotation.Target(java.lang.annotation.ElementType.METHOD) @interface Flag {}";
    let source=format!("package p; public class A extends B {{}} class B {{ @Flag synchronized public @NonNull String read(@Nullable String input) {{return \"x\";}} }} {DECLARED} {DEFAULT} {extra}");
    contains(
        &run(&source, &[]),
        "synchronized public @NonNull String read(@Nullable String input)",
    );
}
#[test]
fn all_default_annotation_members_follow_jdt_member_key_rule() {
    let default="@java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE) @interface Default {boolean enabled() default true;}";
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {default}");
    contains(&run(&source, &[]), "public String read(String input)");
}

#[test]
fn unimplemented_method_correction_applies_target_nullness_default() {
    let source=format!("package p; @Default public class A implements I {{}} interface I {{ @NonNull String read(@NonNull String input); }} {TYPE_USE} {DEFAULT}");
    contains(
        &run_kind(&source, &[], "quickfix", &[]),
        "public String read(String input)",
    );
}
