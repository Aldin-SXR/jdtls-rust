//! Unimplemented-method ports from LocalCorrectionQuickFixTest; other methods remain unported.
mod common;
use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
fn setup() -> (QuickFixTest, std::path::PathBuf) {
    let mut t = QuickFixTest::new();
    let mut options = test_default_options();
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.unusedPrivateMember".into(),
        "error".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.compiler.problem.deadCode".into(),
        "warning".into(),
    );
    let root = t.ws.new_empty_project(&options);
    t.set_ignored_commands(&["Extract.*"]);
    t.ws.create_cu(
        &root,
        "src",
        "test1",
        "E.java",
        "package test1;\npublic interface E {\n    void foo();\n}\n",
    );
    (t, root)
}
#[test]
fn test_unimplemented_methods() {
    let (mut t, root) = setup();
    let uri = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "F.java",
        "package test1;\npublic class F implements E {\n}\n",
    );
    t.assert_code_actions(&uri,&[Expected::new("Add unimplemented methods","package test1;\npublic class F implements E {\n\n    @Override\n    public void foo() {\n        // TODO Auto-generated method stub\n        throw new UnsupportedOperationException(\"Unimplemented method 'foo'\");\n    }\n}\n")]);
}
#[test]
fn test_unimplemented_methods_for_enum() {
    let (mut t, root) = setup();
    let uri = t.ws.create_cu(
        &root,
        "src",
        "test1",
        "F.java",
        "package test1;\npublic enum F implements E {\n}\n",
    );
    t.assert_code_actions(&uri,&[Expected::new("Add unimplemented methods","package test1;\npublic enum F implements E {\n    ;\n\n    @Override\n    public void foo() {\n        // TODO Auto-generated method stub\n        throw new UnsupportedOperationException(\"Unimplemented method 'foo'\");\n    }\n}\n")]);
}
