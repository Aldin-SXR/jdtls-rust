//! Port of `org.eclipse.jdt.ls.core.internal.refactoring.ConvertForLoopTest`.

mod common;

use common::jdtls::{range, test_default_options};
use common::quickfix::{Expected, QuickFixTest};

fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    t.set_selection_test();
    let mut options = test_default_options();
    options.insert("org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotation".to_owned(), "warning".to_owned());
    let root = t.ws.new_empty_project(&options);
    (t, root)
}

#[test]
fn test_convert_for_loop() {
    let (mut t, root) = setup();
    let mut builder = String::new();
    builder.push_str("package test1;\n");
    builder.push_str("import java.util.List;\n");
    builder.push_str("public class E {\n");
    builder.push_str("    void foo(List<String> collection) {\n");
    builder.push_str("    	for (int i=0;i<collection.size();i++) {\n");
    builder.push_str("    		System.out.println(collection.get(i));\n");
    builder.push_str("    	}\n");
    builder.push_str("    }\n");
    builder.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test1", "E.java", &builder);
    let mut builder = String::new();
    builder.push_str("package test1;\n");
    builder.push_str("import java.util.List;\n");
    builder.push_str("public class E {\n");
    builder.push_str("    void foo(List<String> collection) {\n");
    builder.push_str("    	for (String element : collection) {\n");
    builder.push_str("    		System.out.println(element);\n");
    builder.push_str("    	}\n");
    builder.push_str("    }\n");
    builder.push_str("}\n");
    let e = Expected::new("Convert to enhanced 'for' loop", &builder);
    let range = range(4, 5, 4, 5);
    t.assert_code_actions_range(&cu, range, &[e]);
}
