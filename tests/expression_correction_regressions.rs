//! Invalid-operator, standalone-expression and NLS-tag oracle regressions.
mod common;
use common::jdtls::{apply_edits, is_oracle, test_default_options, Workspace};
use common::quickfix::{get_range, quickfix_client_capabilities, QuickFixTest};
use serde_json::{json, Value};
const LOCAL: &str = "Create local variable using expression";
const BIT: &str = "Put bit operations in parentheses";
const NLS: &str = "Remove unnecessary '$NON-NLS$' tag";
fn setup(source: &str, options: &[(&str, &str)]) -> (QuickFixTest, String) {
    let mut t = QuickFixTest::new();
    let mut o = test_default_options();
    for k in ["source", "compliance", "codegen.targetPlatform"] {
        o.insert(format!("org.eclipse.jdt.core.compiler.{k}"), "21".into());
    }
    o.insert(
        "org.eclipse.jdt.core.compiler.problem.nonExternalizedStringLiteral".into(),
        "warning".into(),
    );
    for (k, v) in options {
        o.insert((*k).into(), (*v).into());
    }
    let root = t.ws.new_empty_project(&o);
    let uri = t.ws.create_cu(&root, "src", "p", "A.java", source);
    t.set_only(&["quickfix"]);
    (t, uri)
}
fn action(t: &mut QuickFixTest, uri: &str, title: &str) -> Value {
    let actions = t.evaluate_code_actions(uri);
    let a = actions
        .iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("Missing {title}: {actions:#?}"))
        .clone();
    assert_eq!(a["kind"], "quickfix");
    a
}
fn run_options(source: &str, title: &str, options: &[(&str, &str)]) -> String {
    let (mut t, uri) = setup(source, options);
    let a = action(&mut t, &uri, title);
    t.evaluate_code_action_command(&a)
}
fn run(source: &str, title: &str) -> String {
    run_options(source, title, &[])
}
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn contains(s: &str, fragment: &str) {
    assert!(
        compact(s).contains(&compact(fragment)),
        "Missing {fragment}: {s}"
    );
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
fn negation_moves_around_a_comparison() {
    contains(
        &run(
            "package p; public class A {boolean run(int i){return !i < 10;}}",
            "Put '<' expression in parentheses",
        ),
        "return !(i < 10);",
    );
}
#[test]
fn negation_preserves_operator_comments() {
    let s = run(
        "package p; public class A {boolean run(Object x){return !/*keep*/x instanceof Runnable;}}",
        "Put 'instanceof' in parentheses",
    );
    contains(&s, "return !(/*keep*/x instanceof Runnable);");
}
#[test]
fn parenthesized_invalid_negation_has_no_grouping_proposal() {
    let (mut t, uri) = setup(
        "package p; public class A {boolean run(Object x){return (!x) instanceof Runnable;}}",
        &[],
    );
    t.assert_code_action_not_exists(&uri, "Put 'instanceof' in parentheses");
}
#[test]
fn bitwise_left_side_keeps_all_operands() {
    let source =
        "package p; public class A {boolean run(int a,int b,int c,int d){return a & b & c == d;}}";
    assert_eq!(
        run(source, BIT),
        source.replace("a & b & c == d", "(a & b & c) == d")
    );
}
#[test]
fn bitwise_right_side_keeps_all_operands() {
    let source =
        "package p; public class A {boolean run(int a,int b,int c,int d){return a == b ^ c ^ d;}}";
    assert_eq!(
        run(source, BIT),
        source.replace("a == b ^ c ^ d", "a == (b ^ c ^ d)")
    );
}
#[test]
fn bitwise_both_sides_are_parenthesized() {
    let source =
        "package p; public class A {boolean run(int a,int b,int c,int d){return a & b == c | d;}}";
    assert_eq!(
        run(source, BIT),
        source.replace("a & b == c | d", "(a & b) == (c | d)")
    );
}
#[test]
fn bitwise_insertion_preserves_comments_and_spacing() {
    let source =
        "package p; public class A {boolean run(int a,int b,int c){return a /*left*/ & b != c;}}";
    assert_eq!(
        run(source, BIT),
        source.replace("a /*left*/ & b != c", "(a /*left*/ & b) != c")
    );
}
#[test]
fn primitive_expression_gets_a_local_declaration() {
    contains(
        &run("package p; public class A {void run(){((int)1);}}", LOCAL),
        "int i = (int)1;",
    );
}
#[test]
fn local_name_reserves_parameters_and_later_block_variables() {
    contains(&run("package p; public class A {void run(String string){((String)string); {String string2=null;}}}",LOCAL),"String string3=(String)string;");
}
#[test]
fn local_name_ignores_nested_type_and_anonymous_class_members() {
    contains(&run("package p; public class A {void run(String input){((String)input); class Inner {String string;} Object x=new Object(){String string;};}}",LOCAL),"String string=(String)input;");
}
#[test]
fn local_name_does_not_reserve_fields() {
    contains(
        &run(
            "package p; public class A {String string; void run(String input){((String)input);}}",
            LOCAL,
        ),
        "String string=(String)input;",
    );
}
#[test]
fn expression_fix_uses_parameter_affixes_and_numeric_suffixes() {
    let s = run_options(
        "package p; public class A {void run(String pStringArg){((String)pStringArg);}}",
        LOCAL,
        &[
            ("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p"),
            ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
            ("org.eclipse.jdt.core.codeComplete.localPrefixes", "local"),
        ],
    );
    contains(&s, "String pString2Arg=(String)pStringArg;");
}
#[test]
fn generic_cast_keeps_type_arguments_and_uses_the_type_name() {
    let s = run(
        "package p; public class A {void run(Object input){((java.util.List<String>)input);}}",
        LOCAL,
    );
    assert!(!s.contains("import java.util.List;"), "{s}");
    contains(
        &s,
        "java.util.List<String> list=(java.util.List<String>)input;",
    );
}

#[test]
fn imported_generic_cast_preserves_the_existing_import() {
    let s=run("package p;\nimport java.util.List;\npublic class A {void run(Object input){((List<String>)input);}}",LOCAL);
    assert_eq!(s.matches("import java.util.List;").count(), 1, "{s}");
    contains(&s, "List<String> list=(List<String>)input;");
}

#[test]
fn primitive_parameter_affixes_reserve_the_complete_name() {
    let s = run_options(
        "package p; public class A {void run(int pIArg){((int)pIArg);}}",
        LOCAL,
        &[
            ("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p"),
            ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
        ],
    );
    contains(&s, "int pI2Arg=(int)pIArg;");
}

#[test]
fn invalid_first_affix_falls_back_to_the_next_affix() {
    let s = run_options(
        "package p; public class A {void run(String input){((String)input);}}",
        LOCAL,
        &[
            (
                "org.eclipse.jdt.core.codeComplete.argumentPrefixes",
                ".bad,p",
            ),
            ("org.eclipse.jdt.core.codeComplete.argumentSuffixes", "Arg"),
        ],
    );
    contains(&s, "String pStringArg=(String)input;");
}

#[test]
fn initializer_expression_does_not_reserve_other_initializers() {
    let s = run(
        "package p; public class A {static {String string=null; ((String)null);}}",
        LOCAL,
    );
    contains(&s, "String string=(String)null;");
}

#[test]
fn parameter_prefix_keeps_java_character_case_mapping() {
    let s = run_options(
        "package p; public class A {void run(ßThing input){((ßThing)input);}} class ßThing {}",
        LOCAL,
        &[("org.eclipse.jdt.core.codeComplete.argumentPrefixes", "p")],
    );
    contains(&s, "ßThing pßThing=(ßThing)input;");
}

#[test]
fn parameter_lowercase_keeps_java_single_character_mapping() {
    let s = run(
        "package p; public class A {void run(İThing input){((İThing)input);}} class İThing {}",
        LOCAL,
    );
    contains(&s, "İThing iThing=(İThing)input;");
}

#[test]
fn parameter_names_allow_combining_identifier_characters() {
    let s=run("package p; public class A {void run(I\u{301}Thing input){((I\u{301}Thing)input);}} class I\u{301}Thing {}",LOCAL);
    contains(&s, "I\u{301}Thing i\u{301}Thing=(I\u{301}Thing)input;");
}

#[test]
fn primitive_names_reserve_unicode_case_equivalent_parameters() {
    let s = run(
        "package p; public class A {void run(int İ){((int)İ);}}",
        LOCAL,
    );
    contains(&s, "int j=(int)İ;");
}

#[test]
fn parameter_names_allow_currency_identifier_characters() {
    let s = run(
        "package p; public class A {void run(€Thing input){((€Thing)input);}} class €Thing {}",
        LOCAL,
    );
    contains(&s, "€Thing €Thing=(€Thing)input;");
}

#[test]
fn parameter_names_preserve_supplementary_letter_case() {
    let s=run("package p; public class A {void run(\u{10400}Thing input){((\u{10400}Thing)input);}} class \u{10400}Thing {}",LOCAL);
    contains(&s, "\u{10400}Thing \u{10400}Thing=(\u{10400}Thing)input;");
}
#[test]
fn reference_array_uses_the_reference_dimension_zero_name_rule() {
    contains(
        &run(
            "package p; public class A {void run(Object input){((String[])input);}}",
            LOCAL,
        ),
        "String[] name=(String[])input;",
    );
}
#[test]
fn primitive_array_uses_the_primitive_dimension_zero_name_rule() {
    contains(
        &run(
            "package p; public class A {void run(Object input){((int[])input);}}",
            LOCAL,
        ),
        "int[] i=(int[])input;",
    );
}
#[test]
fn unresolved_cast_still_offers_a_local_variable() {
    contains(
        &run(
            "package p; public class A {void run(Object input){((Missing)input);}}",
            LOCAL,
        ),
        "Missing missing=(Missing)input;",
    );
}
#[test]
fn nls_removal_discards_leading_and_trailing_indent() {
    let source = "package p;\npublic class A {void run(){int i=1; \t//$NON-NLS-1$ \t\n}}\n";
    assert_eq!(run(source, NLS), source.replace(" \t//$NON-NLS-1$ \t", ""));
}
#[test]
fn nls_removal_keeps_comment_text() {
    let source = "package p;\npublic class A {void run(){int i=1; //$NON-NLS-1$ explanation\n}}\n";
    assert_eq!(run(source, NLS), source.replace("//$NON-NLS-1$", "//"));
}
#[test]
fn nls_removal_keeps_a_second_comment_marker() {
    let source =
        "package p;\npublic class A {void run(){int i=1; //$NON-NLS-1$ // explanation\n}}\n";
    assert_eq!(run(source, NLS), source.replace("//$NON-NLS-1$ ", ""));
}
#[test]
fn nls_removal_keeps_a_single_slash_as_comment_text() {
    let source =
        "package p;\npublic class A {void run(){int i=1; //$NON-NLS-1$ / explanation\n}}\n";
    assert_eq!(run(source, NLS), source.replace("//$NON-NLS-1$", "//"));
}
#[test]
fn nls_removal_keeps_an_adjacent_valid_tag() {
    let source = "package p;\npublic class A {String s=\"hello\"; //$NON-NLS-2$ //$NON-NLS-1$\n}\n";
    assert_eq!(run(source, NLS), source.replace(" //$NON-NLS-2$", ""));
}
#[test]
fn nls_removal_at_eof_keeps_following_indent() {
    let source = "package p; public class A {int i=1; //$NON-NLS-1$ \t";
    assert_eq!(run(source, NLS), source.replace(" //$NON-NLS-1$", ""));
}
#[test]
fn deferred_expression_correction_attaches_diagnostics_and_resource_edits() {
    let source = "package p; public class A {void run(){((int)1);}}";
    let (mut t, uri) = setup(source, &[]);
    t.ws.capabilities["textDocument"]["codeAction"]["dataSupport"] = json!(true);
    t.ws.capabilities["textDocument"]["codeAction"]["resolveSupport"] =
        json!({"properties":["edit"]});
    t.ws.capabilities["workspace"]["workspaceEdit"]["resourceOperations"] =
        json!(["create", "rename", "delete"]);
    let a = action(&mut t, &uri, LOCAL);
    assert!(a["edit"].is_null(), "{a}");
    assert!(!a["diagnostics"].as_array().unwrap().is_empty());
    let a = t.ws.request("codeAction/resolve", a);
    assert!(a["edit"]["documentChanges"].is_array(), "{a}");
    contains(
        &apply_edits(source, edits(&a["edit"], &uri)),
        "int i=(int)1;",
    );
}
#[test]
fn unsaved_unicode_crlf_nls_removal_preserves_disk() {
    let disk = "package p; public class A {}";
    let source = "package p;\r\n// 😀 café\r\npublic class A {int i=1; //$NON-NLS-1$\r\n}\r\n";
    let (mut t, uri) = setup(disk, &[]);
    t.diagnostics(&uri);
    t.ws.change(&uri, source);
    let a = action(&mut t, &uri, NLS);
    assert_eq!(
        apply_edits(source, edits(&a["edit"], &uri)),
        source.replace(" //$NON-NLS-1$", "")
    );
    assert_eq!(t.ws.read(&uri), disk);
}
#[test]
fn virtual_documents_offer_all_three_correction_families() {
    if is_oracle() {
        return;
    }
    for uri in [
        "untitled:Expressions.java",
        "inmemory://parity/Expressions.java",
        "file:///tmp/jdtls-parity-missing-expressions/A.java",
    ] {
        for (source, selection, title, expected) in [
            (
                "class A {boolean run(Object x){return !x instanceof Runnable;}}",
                "!x",
                "Put 'instanceof' in parentheses",
                "return !(x instanceof Runnable);",
            ),
            (
                "class A {void run(){((int)1);}}",
                "((int)1)",
                LOCAL,
                "int i=(int)1;",
            ),
            (
                "class A {int i=1; //$NON-NLS-1$\n}",
                "//$NON-NLS-1$",
                NLS,
                "int i=1;\n}",
            ),
        ] {
            let mut ws = Workspace::new();
            ws.capabilities = quickfix_client_capabilities();
            ws.init_options["compilerOptions"] = json!({"org.eclipse.jdt.core.compiler.problem.nonExternalizedStringLiteral":"warning"});
            ws.open_with(uri, source);
            let diagnostics = ws.diagnostics(uri);
            let actions=ws.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":get_range(source,selection),"context":{"diagnostics":diagnostics,"only":["quickfix"]}}));
            let a = actions
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["title"] == title)
                .unwrap_or_else(|| panic!("{actions:#?}"));
            contains(&apply_edits(source, edits(&a["edit"], uri)), expected);
        }
    }
}
