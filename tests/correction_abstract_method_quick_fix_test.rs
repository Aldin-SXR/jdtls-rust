//! All upstream AbstractMethodQuickFixTest fixtures and complete edited-source assertions.
mod common;
use common::jdtls::test_default_options;
use common::quickfix::{Expected, QuickFixTest};
fn check(file: &str, source: &str, expected: &[(&str, &str)]) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&test_default_options());
    let uri = t.ws.create_cu(&root, "src", "test", file, source);
    let expected: Vec<_> = expected
        .iter()
        .map(|(name, source)| Expected::new(name, source))
        .collect();
    t.assert_code_actions(&uri, &expected);
}
#[test]
fn test_abstract_method_in_concrete_class() {
    check("ConcreteClass.java","package test;\npublic class ConcreteClass {\n    public abstract void AbstractMethodInConcreteClass() {\n    }\n}\n",&[
        ("Remove 'abstract' modifier","package test;\npublic class ConcreteClass {\n    public void AbstractMethodInConcreteClass() {\n    }\n}\n"),
        ("Make type 'ConcreteClass' abstract","package test;\npublic abstract class ConcreteClass {\n    public abstract void AbstractMethodInConcreteClass() {\n    }\n}\n"),
        ("Remove method body","package test;\npublic class ConcreteClass {\n    public abstract void AbstractMethodInConcreteClass();\n}\n"),
    ]);
}
#[test]
fn test_abstract_method_with_body() {
    check("TestClass.java","package test;\npublic abstract class TestClass {\n    public abstract void TestMethod() {\n    }\n}\n",&[
        ("Remove 'abstract' modifier","package test;\npublic abstract class TestClass {\n    public void TestMethod() {\n    }\n}\n"),
        ("Remove method body","package test;\npublic abstract class TestClass {\n    public abstract void TestMethod();\n}\n"),
    ]);
}
#[test]
fn test_abstract_method_with_body2() {
    check("TestClass.java","package test;\nabstract class TestClass {\n    public abstract void TestMethod() {}\n}\n",&[
        ("Remove 'abstract' modifier","package test;\nabstract class TestClass {\n    public void TestMethod() {}\n}\n"),
        ("Remove method body","package test;\nabstract class TestClass {\n    public abstract void TestMethod();\n}\n"),
    ]);
}
#[test]
fn test_abstract_method_with_body3() {
    check("TestEnum.java","package test;\n\nenum TestEnum {\n    A {\n        public void TestMethod() {}\n    };\n    public abstract void TestMethod() {}\n}\n",&[
        ("Remove method body","package test;\n\nenum TestEnum {\n    A {\n        public void TestMethod() {}\n    };\n    public abstract void TestMethod();\n}\n"),
    ]);
}
#[test]
fn test_abstract_method_in_enum() {
    check("TestEnum.java","package test;\npublic enum TestEnum {\n    public abstract void TestMethod() {\n    }\n}\n",&[
        ("Remove 'abstract' modifier","package test;\npublic enum TestEnum {\n    public void TestMethod() {\n    }\n}\n"),
        ("Remove method body","package test;\npublic enum TestEnum {\n    public abstract void TestMethod();\n}\n"),
    ]);
}
#[test]
fn test_abstract_method_in_enum2() {
    check(
        "TestEnum.java",
        "package test;\npublic enum TestEnum {\n    public abstract void TestMethod();\n}\n",
        &[(
            "Remove 'abstract' modifier",
            "package test;\npublic enum TestEnum {\n    public void TestMethod() {\n    }\n}\n",
        )],
    );
}
#[test]
fn test_abstract_method_in_enum_without_enum_constants() {
    check("TestEnum.java","package test;\nenum TestEnum {\n    public abstract boolean TestMethod();\n}\n",&[
        ("Remove 'abstract' modifier","package test;\nenum TestEnum {\n    public boolean TestMethod() {\n        return false;\n    }\n}\n"),
    ]);
}
#[test]
fn test_enum_abstract_method_must_be_implementd() {
    check("Animal.java","package test;\npublic enum Animal {\n    CAT {\n        public abstract void makeNoise();\n    };\n    public abstract void makeNoise();\n}\n",&[
        ("Remove 'abstract' modifier","package test;\npublic enum Animal {\n    CAT {\n        public void makeNoise() {\n        }\n    };\n    public abstract void makeNoise();\n}\n"),
    ]);
}
