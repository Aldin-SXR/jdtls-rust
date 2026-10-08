//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.ExtractFieldTest`.
//!
//! `failHelper` and `helper` call `ExtractFieldRefactoring` directly
//! upstream. Here they go through the server's public surface, which runs
//! the same refactoring:
//!
//! * `failHelper`: `java/getRefactorEdit` with `extractField`
//!   (`RefactorProposalUtility.getExtractFieldProposal`) yields no proposal
//!   when `checkInitialConditions` is not OK.
//! * `helper`: the `canEnableSettingDeclareIn*` checks are the
//!   `initializedScopes` that `getInitializeScopes` reports in the extract
//!   field command of an `advancedExtractRefactoringSupport` client; the edit
//!   is the `java/getRefactorEdit` result for `extractField` with that scope
//!   (`setFieldName(guessFieldName())`, `setInitializeIn(scope)`,
//!   `createChange`).

mod common;

use common::jdtls::test_default_options;
use common::quickfix::{evaluate_workspace_edit, get_selection_range, Expected, QuickFixTest};
use serde_json::{json, Value};

const REFACTOR_EXTRACT_FIELD: &str = "refactor.extract.field";

/// `RefactorProposalUtility.InitializeScope`.
#[derive(Clone, Copy)]
enum InitializeScope {
    FieldDeclaration,
    CurrentMethod,
    ClassConstructors,
}

impl InitializeScope {
    fn name(self) -> &'static str {
        match self {
            InitializeScope::FieldDeclaration => "Field declaration",
            InitializeScope::CurrentMethod => "Current method",
            InitializeScope::ClassConstructors => "Class constructors",
        }
    }
}

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let root = t.ws.new_empty_project(&test_default_options());
    t.set_only(&["refactor"]);
    (t, root)
}

/// The `java/getRefactorEdit` `extractField` response for the marked selection.
fn refactor_edit(t: &mut QuickFixTest, cu: &str, initialize_in: Option<InitializeScope>) -> Value {
    let diagnostics = t.diagnostics(cu);
    let range = get_selection_range(&t.ws.read(cu));
    let arguments = match initialize_in {
        Some(scope) => json!([scope.name()]),
        None => json!([]),
    };
    t.ws.request(
        "java/getRefactorEdit",
        json!({
            "command": "extractField",
            "commandArguments": arguments,
            "context": { "textDocument": { "uri": cu }, "range": range, "context": { "diagnostics": diagnostics, "only": ["refactor"] } }
        }),
    )
}

/// `RefactorProposalUtility.getInitializeScopes(refactoring)` for the marked
/// selection, from the extract field command an advanced client receives
/// (`None` when `checkInitialConditions` is not OK).
fn initialize_scopes(cu: &str, source: &str) -> Option<Vec<String>> {
    let mut t = QuickFixTest::new();
    t.ws.init_options["extendedClientCapabilities"]["advancedExtractRefactoringSupport"] = json!(true);
    t.set_selection_test();
    t.set_only(&["refactor"]);
    let root = t.ws.new_empty_project(&test_default_options());
    let path = tower_lsp::lsp_types::Url::parse(cu).unwrap().to_file_path().unwrap();
    let package = path.parent().unwrap().file_name().unwrap().to_str().unwrap().to_owned();
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let uri = t.ws.create_cu(&root, "src", &package, &name, source);
    let actions = t.evaluate_code_actions(&uri);
    let action = actions.iter().find(|a| a["kind"] == REFACTOR_EXTRACT_FIELD)?;
    let arguments = &action["command"]["arguments"];
    assert_eq!(arguments[0], "extractField", "{action:#}");
    Some(arguments[2]["initializedScopes"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect())
}

/// `failHelper(cu)`.
fn fail_helper(t: &mut QuickFixTest, cu: &str) {
    let result = refactor_edit(t, cu, None);
    assert!(result.is_null(), "precondition was supposed to fail: {result:#}");
}

/// `helper(cu, initializeIn, expected)`.
fn helper(t: &mut QuickFixTest, cu: &str, initialize_in: Option<InitializeScope>, expected: &str) -> bool {
    let source = t.ws.read(cu);
    let scopes = initialize_scopes(cu, &source);
    assert!(scopes.is_some(), "activation was supposed to be successful");
    if let Some(scope) = initialize_in {
        if !scopes.unwrap().iter().any(|s| s == scope.name()) {
            return false;
        }
    }
    let result = refactor_edit(t, cu, initialize_in);
    assert!(!result["edit"].is_null(), "{result:#}");
    let actual = evaluate_workspace_edit(&t.ws, &result["edit"]).unwrap();
    assert_eq!(expected, actual);
    true
}

#[test]
fn test_extract_to_field_disabled_in_constructor_invocation() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	A(int x, int y){\n");
    buf.push_str("		this(/*]*/x + y/*[*/);\n");
    buf.push_str("	};\n");
    buf.push_str("	A(int x){\n");
    buf.push_str("	};\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_field_declaration() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private int x = /*]*/1/*[*/;\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_null_literal() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void test() {\n");
    buf.push_str("		Object object = /*]*/null/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_array_initializer() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void test() {\n");
    buf.push_str("		int[] array = new int[] /*]*/{ 1 + 2 }/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_non_parenthesized_assignment() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void test() {\n");
    buf.push_str("		int x, y;");
    buf.push_str("		int z = y = /*]*/ x = 1 + 2 /*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_void() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void test() {\n");
    buf.push_str("		/*]*/print()/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("	public void print() {\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_for_initializer() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void test() {\n");
    buf.push_str("		int i;\n");
    buf.push_str("		for (/*]*/i = 0/*[*/; i < 10; i++);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_interface() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("interface E {\n");
    buf.push_str("	default void print(int x) {\n");
    buf.push_str("		/*]*/x++/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_disabled_in_method_with_local_type() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	void print(int x) {\n");
    buf.push_str("		class Local {}\n");
    buf.push_str("		Local local = /*]*/new Local()/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    fail_helper(&mut t, &cu);
}

#[test]
fn test_extract_to_field_method() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private int i;\n\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		this.i = 0;\n");
    buf.push_str("        int x= /*]*/this.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private int i = 0;\n\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		int x= /*]*/this.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private int i;\n\n");
    buf.push_str("    E() {\n");
    buf.push_str("        this.i = 0;\n");
    buf.push_str("    }\n\n");
    buf.push_str("    void m(int i){\n");
    buf.push_str("		int x= /*]*/this.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), &buf));
}

#[test]
fn test_extract_to_field_static_method() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	static void m(int i){\n");
    buf.push_str("		int x= /*]*/0/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private static int i;\n\n");
    buf.push_str("    static void m(int i){\n");
    buf.push_str("		E.i = 0;\n");
    buf.push_str("        int x= /*]*/E.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private static int i = 0;\n\n");
    buf.push_str("    static void m(int i){\n");
    buf.push_str("		int x= /*]*/E.i/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    assert!(!helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), ""));
}

#[test]
fn test_extract_to_field_constructor() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	E(){\n");
    buf.push_str("		Object x = /*]*/new Object()/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object;\n\n");
    buf.push_str("    E(){\n");
    buf.push_str("		object = new Object();\n");
    buf.push_str("        Object x = /*]*/object/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object = new Object();\n\n");
    buf.push_str("    E(){\n");
    buf.push_str("		Object x = /*]*/object/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    assert!(!helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), ""));
}

#[test]
fn test_extract_to_field_lambda_expression() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Arrays;\n\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		Arrays.asList(1, 2).stream().map((number) -> /*]*/number * number/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("import java.util.Arrays;\n\n");
    buf.push_str("class E {\n");
    buf.push_str("	private int i;\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("		Arrays.asList(1, 2).stream().map((number) -> /*]*/{\n");
    buf.push_str("            i = number * number;\n");
    buf.push_str("            return i;\n");
    buf.push_str("        }/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    assert!(!helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), ""));
    assert!(!helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), ""));
}

#[test]
fn test_extract_to_field_lambda_expression_return_void() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		new Thread(() -> /*]*/new Object()/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object;\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("		new Thread(() -> /*]*/{\n");
    buf.push_str("            object = new Object();\n");
    buf.push_str("        }/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object = new Object();\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("		new Thread(() -> /*]*/object/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object;\n\n");
    buf.push_str("    E() {\n");
    buf.push_str("        object = new Object();\n");
    buf.push_str("    }\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("		new Thread(() -> /*]*/object/*[*/);\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), &buf));
}

#[test]
fn test_extract_to_field_anonymous_class() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		new Runnable() {\n");
    buf.push_str("			public void run() {\n");
    buf.push_str("				Object x = /*]*/new Object()/*[*/;\n");
    buf.push_str("			}\n");
    buf.push_str("		};\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		new Runnable() {\n");
    buf.push_str("			private Object object;\n\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("				object = new Object();\n");
    buf.push_str("                Object x = /*]*/object/*[*/;\n");
    buf.push_str("			}\n");
    buf.push_str("		};\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		new Runnable() {\n");
    buf.push_str("			private Object object = new Object();\n\n");
    buf.push_str("            public void run() {\n");
    buf.push_str("				Object x = /*]*/object/*[*/;\n");
    buf.push_str("			}\n");
    buf.push_str("		};\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    assert!(!helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), ""));
}

#[test]
fn test_extract_to_field_standalone_statement() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	public void f(){\n");
    buf.push_str("		/*]*/new Object()/*[*/;\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &buf);
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object;\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("		/*]*/object = new Object();\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    let e1 = Expected::with_kind("Extract to field", &buf, REFACTOR_EXTRACT_FIELD);
    t.assert_code_actions(&cu, &[e1]);
    assert!(helper(&mut t, &cu, Some(InitializeScope::CurrentMethod), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object = new Object();\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::FieldDeclaration), &buf));
    let mut buf = String::new();
    buf.push_str("package test1;\n");
    buf.push_str("class E {\n");
    buf.push_str("	private Object object;\n\n");
    buf.push_str("    E() {\n");
    buf.push_str("        object = new Object();\n");
    buf.push_str("    }\n\n");
    buf.push_str("    public void f(){\n");
    buf.push_str("	}\n");
    buf.push_str("}\n");
    assert!(helper(&mut t, &cu, Some(InitializeScope::ClassConstructors), &buf));
}
