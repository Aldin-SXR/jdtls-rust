//! Port of `org.eclipse.jdt.ls.core.internal.handlers.SignatureHelpHandlerTest`.
//!
//! `setup()` imports `eclipse/hello` with `java.signatureHelp.enabled = true`
//! and `java.signatureHelp.description.enabled = false`, like the mocked
//! preferences upstream; tests that enable descriptions do so before the
//! server starts.

mod common;
use common::jdtls::Workspace;
use serde_json::{json, Value};

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.settings = json!({
        "java": {
            "signatureHelp": { "enabled": true, "description": { "enabled": false } },
            "maven": { "defaultMojoExecutionAction": "ignore" },
            "import": { "maven": { "enabled": true } }
        }
    });
    ws
}

/// `when(isSignatureHelpDescriptionEnabled()).thenReturn(true)`.
fn enable_description(ws: &mut Workspace) {
    ws.settings["java"]["signatureHelp"]["description"]["enabled"] = json!(true);
}

/// `sourceFolder.createPackageFragment(pkg).createCompilationUnit(name, content)` in `hello/src`.
fn create_cu(ws: &mut Workspace, pkg: &str, name: &str, content: &str) -> String {
    let root = ws.project_root("hello");
    ws.create_cu(&root, "src", pkg, name, content)
}

fn get_signature_help(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Value {
    ws.request(
        "textDocument/signatureHelp",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    )
}

fn signatures(help: &Value) -> Vec<Value> {
    help["signatures"].as_array().cloned().unwrap_or_default()
}

/// `help.getSignatures().get(help.getActiveSignature())`.
fn active(help: &Value) -> Value {
    let index = help["activeSignature"].as_u64().unwrap_or_else(|| panic!("no active signature in {help:#}"));
    signatures(help)[index as usize].clone()
}

fn active_label(help: &Value) -> String {
    active(help)["label"].as_str().unwrap_or_default().to_owned()
}

fn active_parameter(help: &Value) -> u64 {
    help["activeParameter"].as_u64().unwrap_or_else(|| panic!("no active parameter in {help:#}"))
}

fn matches(label: &str, pattern: &str) -> bool {
    regex_full_match(pattern, label)
}

/// `String.matches`: the whole input must match the pattern.
fn regex_full_match(pattern: &str, input: &str) -> bool {
    regex::Regex::new(&format!("^(?:{pattern})$")).unwrap().is_match(input)
}

/// `AbstractCompilationUnitBasedTest.findCompletionLocation`.
fn find_completion_location(content: &str, complete_behind: &str) -> (u32, u32) {
    let offset = content.rfind(complete_behind).unwrap() + complete_behind.len();
    let before = &content[..offset];
    let line = before.matches('\n').count() as u32;
    let column = before.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
    (line, column)
}

#[test]
fn test_signature_help_single_method() {
    let mut ws = setup();
    enable_description(&mut ws);
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   /** This is a method */\n",
        "   public int foo(String s) { }\n",
        "   public int bar(String s) { this.foo() }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 39);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert_eq!("foo(String s) : int", active_label(&help));
    assert!(active(&help)["documentation"].as_str().unwrap().len() > 0);
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_multiple_methods() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public int foo(String s) { }\n",
        "   public int foo(int s) { }\n",
        "   public int foo(int s, String s) { }\n",
        "   public int bar(String s) { this.foo(2,  ) }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 5, 42);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len());
    assert_eq!(1, active_parameter(&help));
    assert_eq!(active_label(&help), "foo(int s, String s) : int");
}

#[test]
fn test_signature_help_binary() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public int bar(String s) { System.out.println(  }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 2, 50);
    assert!(help.is_object());
    assert!(signatures(&help).len() >= 10);
    assert!(active_label(&help) == "println() : void");
    let help2 = get_signature_help(&mut ws, &cu, 2, 49);
    assert_eq!(signatures(&help).len(), signatures(&help2).len());
    assert_eq!(help["activeSignature"], help2["activeSignature"]);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1980
#[test]
fn test_signature_help_double() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "    public E(String s) {\n",
        "        this.unique(\n",
        "    }\n",
        "    public void unique(double d) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 20);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert_eq!(0, active_parameter(&help));
    assert_eq!(active_label(&help), "unique(double d) : void");
}

#[test]
fn test_signature_help_invalid() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public int bar(String s) { if (  }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 2, 34);
    assert!(help.is_object());
    assert_eq!(0, signatures(&help).len());
}

#[test]
fn test_signature_help_end_of_doc() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public int bar(String s) {  }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 2);
    assert!(help.is_object());
    assert_eq!(0, signatures(&help).len());
}

// See https://github.com/eclipse/eclipse.jdt.ls/pull/1015#issuecomment-487997215
#[test]
fn test_signature_help_parameters() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public boolean bar() {\n",
        "     foo(\"\",)\n",
        "     return true;\n",
        "   }\n",
        "   public void foo(String s) {}\n",
        "   public void foo(String s, boolean bar) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 12);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert_eq!(active_label(&help), "foo(String s, boolean bar) : void");
}

#[test]
fn test_signature_help_active_signature() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     foo(\"a\",\"b\");\n",
        "   }\n",
        "   public void foo(String s) {}\n",
        "   public void foo(String s, String b) {}\n",
        "   public void foo() {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 12);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len());
    assert_eq!(active_label(&help), "foo(String s, String b) : void");
}

#[test]
fn test_signature_help_constructor() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     new RuntimeException()\n",
        "   }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 26);
    assert!(help.is_object());
    assert_eq!(4, signatures(&help).len());
    assert_eq!(active_label(&help), "RuntimeException()");
}

#[test]
fn test_signature_help_constructor2() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     new String(,);\n",
        "   }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 16);
    assert!(signatures(&help).len() > 0);
    assert!(matches(&active_label(&help), r"String\(byte\[\] \w+, Charset \w+\)"), "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 3, 17);
    assert!(signatures(&help).len() > 0);
    assert!(matches(&active_label(&help), r"String\(byte\[\] \w+, Charset \w+\)"), "{help:#}");
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_constructor_parameters() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     new RuntimeException(\"t\", )\n",
        "   }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 31);
    assert!(help.is_object());
    assert_eq!(4, signatures(&help).len());
    assert!(matches(&active_label(&help), r"RuntimeException\(String \w+, Throwable \w+\)"), "{help:#}");
}

#[test]
fn test_signature_help_constructor_parameters2() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     new RuntimeException(\"foo\")\n",
        "   }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 31);
    assert!(help.is_object());
    assert_eq!(4, signatures(&help).len());
    assert!(matches(&active_label(&help), r"RuntimeException\(String \w+\)"), "{help:#}");
}

#[test]
fn test_signature_help_constructor_parameters3() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.text.StringCharacterIterator;\n",
        "public class E {\n",
        "   public void bar() {\n",
        "     new StringCharacterIterator(\"\", 0,  , 2);\n",
        "   }\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 39);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len());
    // StringCharacterIterator(String arg0, int arg1, int arg2, int arg3)
    assert!(
        matches(&active_label(&help), r"StringCharacterIterator\(String \w+, int \w+, int \w+, int \w+\)"),
        "{help:#}"
    );
}

#[test]
fn test_signature_help_javadoc() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "/**\n",
        " * @see String#substring()\n",
        " */\n",
        "   public int test() {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 25);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert!(matches(&active_label(&help), r"substring\(\w+ \w+\) : String"), "{help:#}");
}

// See https://github.com/redhat-developer/vscode-java/issues/1258
#[test]
fn test_signature_help_javadoc_original() {
    let mut ws = setup();
    enable_description(&mut ws);
    let content = concat!(
        "package test1;\n",
        "import java.util.LinkedList;\n",
        "import org.sample.MyList;\n",
        "public class E {\n\n",
        "	void test() {\n",
        "		MyList<String> l = new LinkedList<>();\n",
        "		l.add(\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let (line, column) = find_completion_location(content, "l.add(");
    let help = get_signature_help(&mut ws, &cu, line, column);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len(), "{help:#}");
    let signature = active(&help);
    assert!(signature["label"] == "add(String e) : boolean", "{help:#}");
    let documentation = signature["documentation"].as_str().unwrap_or_default();
    assert_eq!(" Test ", documentation);
}

#[test]
fn test_signature_help_varargs() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.Arrays;\n",
        "public class E {\n",
        "	public static void main(String[] args) {\n",
        "		Arrays.asList(1,2,3);\n",
        "		demo(\"1\", \"2\",\"3\" )\n",
        "	}\n",
        "	public static void demo (String s, String... s2) {\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 21);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert!(active_label(&help).starts_with("asList(T... "), "{help:#}");
    let help = get_signature_help(&mut ws, &cu, 5, 19);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert!(active_label(&help) == "demo(String s, String... s2) : void", "{help:#}");
}

#[test]
fn test_signature_help_varargs2() {
    let mut ws = setup();
    let cu = ws.class_uri("hello", "test1.Varargs");
    let help = get_signature_help(&mut ws, &cu, 4, 16);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "run(Class<?> clazz, String... args) : void", "{help:#}");
}

#[test]
fn test_signature_help_varargs3() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(String... args) {}\n",
        "	public void foo(Integer a, String... args) {}\n",
        "	public void bar() {\n",
        "		foo( , args);",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 5, 6);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert!(active_label(&help) == "foo(Integer a, String... args) : void", "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 5, 9);
    assert!(help.is_object());
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_varargs4() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(String... args) {}\n",
        "	public void foo(Integer a, String... args) {}\n",
        "	public void bar() {\n",
        "		foo( , \"()\", \"\");",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 5, 6);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert!(active_label(&help) == "foo(Integer a, String... args) : void", "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 5, 11);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert!(active_label(&help) == "foo(Integer a, String... args) : void", "{help:#}");
    assert_eq!(1, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 5, 16);
    assert!(help.is_object());
    assert_eq!(2, signatures(&help).len());
    assert!(active_label(&help) == "foo(Integer a, String... args) : void", "{help:#}");
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_bracket() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(Integer a, String... args) {}\n",
        "	public void bar() {\n",
        "		foo(1, \"()\");",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 11);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert!(active_label(&help) == "foo(Integer a, String... args) : void", "{help:#}");
    assert_eq!(1, active_parameter(&help));
}

fn println_string_test(content: &str) {
    let mut ws = setup();
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 23);
    assert!(help.is_object());
    assert!(active_label(&help).starts_with("println(String"), "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_bracket2() {
    println_string_test(concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		System.out.println(\"{}\");\n",
        "	}\n",
        "}\n",
    ));
}

#[test]
fn test_signature_help_bracket3() {
    println_string_test(concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		System.out.println(\"()\");\n",
        "	}\n",
        "}\n",
    ));
}

#[test]
fn test_signature_help_bracket4() {
    println_string_test(concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		System.out.println(\"[]\");\n",
        "	}\n",
        "}\n",
    ));
}

#[test]
fn test_signature_help_diamond() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.HashMap;\n",
        "public class E {\n",
        "	public void foo(Object o1) {}\n",
        "	public void foo(Object o1, Object o2) {}\n",
        "	public void bar() {\n",
        "		foo(new HashMap<String, String>());\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 6, 27);
    assert!(help.is_object());
    assert!(active_label(&help) == "foo(Object o1) : void", "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_comma() {
    println_string_test(concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		System.out.println(\",\");\n",
        "	}\n",
        "}\n",
    ));
}

#[test]
fn test_signature_help_complex_args() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(Integer[] a, Character[] b) {}\n",
        "	public void bar() {\n",
        "		foo(new Integer[]{1, 2, 3}, new Character[]{'a', 'b', 'c'});\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    for (character, expected) in [(12, 0), (18, 0), (25, 0), (40, 1), (44, 1), (54, 1)] {
        let help = get_signature_help(&mut ws, &cu, 4, character);
        assert!(help.is_object());
        assert_eq!(1, signatures(&help).len(), "at {character}: {help:#}");
        assert_eq!(expected, active_parameter(&help), "at {character}: {help:#}");
    }
}

#[test]
fn test_signature_help_default_constructor() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		F f = new F();\n",
        "	}\n",
        "}\n",
        "class F {}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 14);
    assert!(help.is_object());
    assert!(active_label(&help) == "F()", "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_constructor_invocation() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public E(int a) {\n",
        "		this(1, 1);\n",
        "	}\n",
        "	public E(int a, int b) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 7);
    assert!(active_label(&help) == "E(int a, int b)", "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 3, 10);
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_record() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/java16"]);
    let unit = ws.class_uri("java16", "foo.bar.Bar");
    let help = get_signature_help(&mut ws, &unit, 11, 10);
    assert!(help.is_object());
    assert!(
        active_label(&help) == "Edge(int fromNodeId, int toNodeId, Object fromPoint, Object toPoint, double length, Object profile)",
        "{help:#}"
    );
}

#[test]
fn test_signature_help_super_constructor_invocation() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E extends F {\n",
        "	public E() {\n",
        "		super(1, 2);\n",
        "	}\n",
        "}\n",
        "class F {\n",
        "	public F(int a, int b) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 11);
    assert!(help.is_object());
    assert!(active_label(&help) == "F(int a, int b)", "{help:#}");
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_super_method() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E extends F {\n",
        "	public void foo() {\n",
        "		E.super.equals(1);\n",
        "	}\n",
        "}\n",
        "class F {}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 17);
    assert!(help.is_object());
    assert!(active_label(&help).starts_with("equals(Object"), "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_different_declaring_type() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public String foo(String a) {return null;}\n",
        "	public F get() {return null;}\n",
        "	public void bar() {\n",
        "		F f = get();\n",
        "		if (f instanceof G) {\n",
        "			((G) f).foo();\n",
        "		}\n",
        "	}\n",
        "}\n",
        "class F {\n",
        "	public String foo() {return null;}\n",
        "}\n",
        "class G extends F {\n",
        "	public String foo() {return null;}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 7, 15);
    assert!(help.is_object());
    // There should really only be one signature in this list, though there are two as a result of
    // https://github.com/eclipse-jdtls/eclipse.jdt.ls/pull/3073
    assert_eq!(2, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "foo() : String", "{help:#}");
}

#[test]
fn test_signature_help_method_chain() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public E setFoo() {\n",
        "		return this;\n",
        "	}\n",
        "	public E setBar() {\n",
        "		return this;\n",
        "	}\n",
        "	public void foo() {\n",
        "		setFoo().setBar();\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 9, 9);
    assert!(help.is_object());
    assert!(active_label(&help) == "setFoo() : E", "{help:#}");

    let help = get_signature_help(&mut ws, &cu, 9, 18);
    assert!(help.is_object());
    assert!(active_label(&help) == "setBar() : E", "{help:#}");
}

#[test]
fn test_signature_help_call_from_method_name() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(Integer a, String... args) {}\n",
        "	public void bar() {\n",
        "		foo(1, \"()\");",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 4);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert_eq!(2, active_parameter(&help));
}

#[test]
fn test_signature_help_enclosing_methods() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(String a) {}\n",
        "	public void bar() {\n",
        "		foo(new String());",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 10);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len());
    assert!(active_label(&help) == "foo(String a) : void", "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 4, 17);
    assert!(help.is_object());
    assert!(active_label(&help) == "String()", "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_text_block() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/java16"]);
    let root = ws.project_root("java16");
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void bar() {\n",
        "		System.out.println(\"\"\"\n",
        "			(,{,[],})\"\"\");\n",
        "	}\n",
        "}\n",
    );
    let cu = ws.create_cu(&root, "src/main/java", "foo.bar", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 10);
    assert!(help.is_object());
    assert!(active_label(&help).starts_with("println(String"), "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_lambda() {
    let mut ws = setup();
    let cu = ws.class_uri("hello", "test1.SignatureHelp");
    let help = get_signature_help(&mut ws, &cu, 8, 14);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "test(Function<String,String> f) : void", "{help:#}");
}

#[test]
fn test_signature_help_lambda2() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.Arrays;\n",
        "public class E {\n",
        "	public static void main(String[] args) {\n",
        "		 Arrays.stream(args).filter(a -> a.length() > 0).toArray(String[]::new);\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 26);
    assert_eq!(1, active_parameter(&help), "{help:#}");

    let help = get_signature_help(&mut ws, &cu, 4, 30);
    assert_eq!(0, active_parameter(&help), "{help:#}");
}

#[test]
fn test_signature_help_generic_types() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.HashMap;\n",
        "import java.util.Map;\n",
        "public class E {\n",
        "	public foo() {\n",
        "		 Map<String, Object> map = new HashMap<>();\n",
        "		 map.put(\"key\", \"value\");\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 6, 11);
    assert!(matches(&active_label(&help), r"put\(String \w+\, Object \w+\) : Object"), "{help:#}");
    assert_eq!(0, active_parameter(&help));

    let help = get_signature_help(&mut ws, &cu, 6, 18);
    assert_eq!(1, active_parameter(&help));
}

#[test]
fn test_signature_help_generic_types2() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.List;\n",
        "import java.util.stream.Collectors;\n",
        "public class E {\n",
        "	public foo(List<String> foo) {\n",
        "		String s = foo.stream().map(m -> m.toString()).collect(Collectors.joining(\", \"));\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 5, 57);
    assert!(matches(&active_label(&help), r"collect\(Collector<\? super String,A,R> \w+\) : R"), "{help:#}");
    assert_eq!(0, active_parameter(&help));
}

#[test]
fn test_signature_help_generic_types3() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "import java.util.Map;\n",
        "public class E {\n",
        "	public foo() {\n",
        "		Map<Object, Object> a = null;\n",
        "		for (Map.Entry<Object, Object> b : a.entrySet()) {\n",
        "			b.getKey().toString();\n",
        "		}\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 6, 12);
    assert!(active_label(&help) == "getKey() : Object", "{help:#}");
}

#[test]
fn test_signature_help_parameter_types() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public static void main(String[] args) {\n",
        "		 new RuntimeException(new Exception(),)\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 40);
    assert!(help.is_object());
    assert_eq!(4, signatures(&help).len(), "{help:#}");
    assert!(help["activeParameter"].is_null(), "{help:#}");
}

#[test]
fn test_signature_help_parameter_object() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public static void main(String[] args) {\n",
        "		 Object foo = new Object();\n",
        "		 System.err.println(foo);\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 4, 23);
    assert!(help.is_object());
    assert_eq!(10, signatures(&help).len(), "{help:#}");
    assert!(!help["activeParameter"].is_null());
    assert!(matches(&active_label(&help), r"println\(Object \w+\) : void"), "{help:#}");
}

#[test]
fn test_signature_help_skip() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public foo() {\n",
        "		new Object()\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 14);
    assert_eq!(0, signatures(&help).len(), "{help:#}");
}

#[test]
fn test_signature_help_invalid_ast() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "class X {\n",
        "	public static void main(string[] args) {\n",
        "\n",
        "	}\n",
        "\n",
        "	static void fun() {\n",
        "		int a\n",
        "		for (l < 10) {\n",
        "\n",
        "		}\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "X.java", content);
    let help = get_signature_help(&mut ws, &cu, 2, 37);
    assert_eq!(0, signatures(&help).len(), "{help:#}");
}

#[test]
fn test_signature_help_nested_invocation() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	/**\n",
        "	 * foo\n",
        "	 */\n",
        "	public String foo() {return \"\";}\n",
        "	/**\n",
        "	 * bar\n",
        "	 */\n",
        "	public void bar(String a) {}\n",
        "	public test() {\n",
        "		bar(foo());\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 11, 10);
    assert_eq!(1, signatures(&help).len(), "{help:#}");
    assert_eq!("foo() : String", active_label(&help));
    let help = get_signature_help(&mut ws, &cu, 11, 11);
    assert_eq!("bar(String a) : void", active_label(&help));
    assert_eq!(1, signatures(&help).len());
}

#[test]
fn test_signature_help_string_literal() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo(String p, int x) {\n",
        "		 foo(\"(\" , 1)\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    for character in 7..=14 {
        test_string_literal(&mut ws, &cu, 3, character);
    }
}

fn test_string_literal(ws: &mut Workspace, cu: &str, line: u32, character: u32) {
    let help = get_signature_help(ws, cu, line, character);
    assert!(help.is_object());
    // see https://bugs.eclipse.org/bugs/show_bug.cgi?id=575149
    // assertEquals(1, help.getSignatures().size());
    assert!(!help["activeParameter"].is_null(), "at {character}: {help:#}");
    assert!(active_label(&help) == "foo(String p, int x) : void", "at {character}: {help:#}");
}

#[test]
fn test_signature_help_assert_equals() {
    let mut ws = setup();
    ws.import_projects(&["maven/classpathtest"]);
    let root = ws.project_root("classpathtest");
    let content = concat!(
        "package test1;\n",
        "import static org.junit.Assert.assertEquals;\n",
        "public class E {\n",
        "	public static void main(String[] args) {\n",
        "		 long num = 1;\n",
        "		 assertEquals(num,num)\n",
        "	}\n",
        "}\n",
    );
    let cu = ws.create_cu(&root, "src/test/java", "test1", "E.java", content);
    for character in 16..=22 {
        test_assert_equals(&mut ws, &cu, 5, character);
    }
}

fn test_assert_equals(ws: &mut Workspace, cu: &str, line: u32, character: u32) {
    let help = get_signature_help(ws, cu, line, character);
    assert!(help.is_object());
    assert_eq!(12, signatures(&help).len(), "at {character}: {help:#}");
    assert!(!help["activeParameter"].is_null());
    assert!(active_label(&help) == "assertEquals(long expected, long actual) : void", "at {character}: {help:#}");
}

// https://github.com/redhat-developer/vscode-java/issues/2097
// (`testSignatureHelpConstructor`; `test_signature_help_constructor` is taken by
// `testSignatureHelp_constructor`.)
#[test]
fn test_signature_help_constructor_vscode_java_2097() {
    let mut ws = setup();
    let cu = ws.class_uri("hello", "test1.SignatureHelp2097");
    let help = get_signature_help(&mut ws, &cu, 10, 51);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "SignatureHelp2097(String name)", "{help:#}");
    let help = get_signature_help(&mut ws, &cu, 11, 53);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "SignatureHelp2097(String name)", "{help:#}");
}

// https://github.com/redhat-developer/vscode-java/issues/2097
#[test]
fn test_signature_help_method() {
    let mut ws = setup();
    let cu = ws.class_uri("hello", "test1.SignatureHelp2097");
    let help = get_signature_help(&mut ws, &cu, 16, 42);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "success(String msg, Object data) : Boolean", "{help:#}");
    let help = get_signature_help(&mut ws, &cu, 18, 39);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "fail(Object data) : Boolean", "{help:#}");
    let help = get_signature_help(&mut ws, &cu, 23, 31);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len(), "{help:#}");
    assert!(active_label(&help) == "fail(String msg, Object data) : Boolean", "{help:#}");
}

const DESCRIPTION_SOURCE: &str = concat!(
    "package test1;\n",
    "public class E {\n",
    "	/**\n",
    "	 * This is an API.\n",
    "	 */\n",
    "	public void foo(String s) {\n",
    "		 foo(null)\n",
    "	}\n",
    "}\n",
);

#[test]
fn test_signature_help_description_disabled() {
    let mut ws = setup();
    let cu = create_cu(&mut ws, "test1", "E.java", DESCRIPTION_SOURCE);
    let help = get_signature_help(&mut ws, &cu, 6, 7);
    assert!(help.is_object());
    assert!(active(&help)["documentation"].is_null(), "{help:#}");
}

#[test]
fn test_signature_help_description_enabled() {
    let mut ws = setup();
    enable_description(&mut ws);
    let cu = create_cu(&mut ws, "test1", "E.java", DESCRIPTION_SOURCE);
    let help = get_signature_help(&mut ws, &cu, 6, 7);
    assert!(help.is_object());
    let documentation = active(&help)["documentation"].as_str().unwrap_or_default().to_owned();
    assert_eq!("This is an API.", documentation.trim());
}

#[test]
fn test_signature_help_erasure_type() {
    let mut ws = setup();
    enable_description(&mut ws);
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public void foo() {\n",
        "		new V<String>();\n",
        "	}\n",
        "}\n",
        "class V<T> {\n",
        "	/** hi */\n",
        "	public V() {}\n",
        "	private V(String a) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    let help = get_signature_help(&mut ws, &cu, 3, 16);
    assert!(help.is_object());
    assert_eq!(1, signatures(&help).len(), "{help:#}");
    let documentation = active(&help)["documentation"].as_str().unwrap_or_default().to_owned();
    assert_eq!("hi", documentation.trim());
}

#[test]
fn test_signature_help_in_class_file() {
    let mut ws = setup();
    let uri = "jdt://contents/java.base/java.lang/String.class";
    let sh = get_signature_help(&mut ws, uri, 10, 10);
    assert!(sh.is_object(), "{sh:#}");
}

#[test]
#[ignore = "upstream selects the first raw CompletionProposalRequestor proposal directly; that ordering is not the first LSP completion item. The public onDidSelect/signature-help flow is covered in completion_regressions"]
fn test_signature_help_for_selected_completion_proposal() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class E {\n",
        "	public foo() {\n",
        "		new String()\n",
        "	}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "E.java", content);
    // Upstream runs the completion engine at `new String(|` and stores its first
    // proposal as `CompletionHandler.selectedProposal`; its description is one of:
    let unnamed_result = "String(byte[] arg0, int arg1, int arg2, Charset arg3)";
    let named_result = "String(byte[] bytes, int offset, int length, Charset charset)";
    // These really should not be acceptable, I'm just testing why the CI is getting weird answers
    let weird1 = "String(byte[] [arg0, int arg1, int arg2, Charset arg3]";
    let weird2 = "String(byte[] [bytes, int offset, int length, Charset charset])";
    let l = [unnamed_result, named_result, weird1, weird2];
    let offset = content.find("new String(").unwrap() + "new String(".len();
    let completion = ws.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": cu }, "position": { "line": 3, "character": offset - content[..offset].rfind('\n').unwrap() - 1 } }),
    );
    let items = completion["items"].as_array().cloned().unwrap_or_default();
    let first = items.first().cloned().unwrap_or(Value::Null);
    ws.request("workspace/executeCommand", first["command"].clone());
    let from_proposal = first["label"].as_str().unwrap_or_default();
    assert!(l.contains(&from_proposal));

    // The result from help signatures seems to vary via a race condition.
    // I am not competent enough to fix this test or impl at this time ;)
    let help = get_signature_help(&mut ws, &cu, 3, 13);
    let from_help_signatures = active_label(&help);
    assert!(l.contains(&from_help_signatures.as_str()));
}

#[test]
fn test_signature_help_overloads() {
    let mut ws = setup();
    let content = concat!(
        "package test1;\n",
        "public class Car extends Vehicle {\n",
        "   public void speed(int x, int y) {}\n",
        "   public static void main(int [] args) {\n",
        "      Car test = new Car();\n",
        "      test.speed();\n",
        "   }\n",
        "}\n",
        "class Vehicle extends MovingObject {\n",
        "   public void speed(int x) {}\n",
        "}\n",
        "class MovingObject {\n",
        "   public void speed(int x, int y, int z) {}\n",
        "}\n",
    );
    let cu = create_cu(&mut ws, "test1", "Car.java", content);
    let help = get_signature_help(&mut ws, &cu, 5, 17);
    assert!(help.is_object());
    assert_eq!(3, signatures(&help).len(), "{help:#}");
}
