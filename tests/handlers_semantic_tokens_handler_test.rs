//! Port of `org.eclipse.jdt.ls.core.internal.handlers.SemanticTokensHandlerTest`.

mod common;
use common::jdtls::{test_default_options, Workspace};
use serde_json::{json, Value};

/// `SemanticTokensHandler.legend()` token types, in legend order.
const TOKEN_TYPES: &[&str] = &[
    "namespace", "class", "interface", "enum", "enumMember", "type", "typeParameter", "method", "property", "variable",
    "parameter", "modifier", "keyword", "annotation", "annotationMember", "record", "recordComponent",
];

/// `SemanticTokensHandler.legend()` token modifiers, in legend order.
const TOKEN_MODIFIERS: &[&str] = &[
    "abstract", "static", "readonly", "deprecated", "declaration", "documentation", "public", "private", "protected",
    "native", "generic", "typeArgument", "importDeclaration", "constructor",
];

fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/semantic-tokens"]);
    set_options(&mut ws);
    ws
}

/// `TestOptions.getDefaultOptions()` + `JavaCore.setComplianceOptions("17", options)`.
fn set_options(ws: &mut Workspace) {
    let mut options = test_default_options();
    for key in [
        "org.eclipse.jdt.core.compiler.compliance",
        "org.eclipse.jdt.core.compiler.source",
        "org.eclipse.jdt.core.compiler.codegen.targetPlatform",
    ] {
        options.insert(key.to_owned(), "17".to_owned());
    }
    let root = ws.project_root("semantic-tokens");
    ws.set_project_options(&root, &options);
}

/// `addTestLibraryToClasspath`: `foo.jar` with `foo-sources.jar` attached.
fn add_test_library_to_classpath(ws: &mut Workspace) {
    let root = ws.project_root("semantic-tokens");
    std::fs::write(root.join(".classpath"), concat!(
        "<classpath>",
        "<classpathentry kind=\"src\" path=\"src/main/java\"/>",
        "<classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/>",
        "<classpathentry kind=\"con\" path=\"org.eclipse.m2e.MAVEN2_CLASSPATH_CONTAINER\"/>",
        "<classpathentry kind=\"lib\" path=\"foo.jar\" sourcepath=\"foo-sources.jar\"/>",
        "<classpathentry kind=\"output\" path=\"target/classes\"/>",
        "</classpath>",
    )).unwrap();
}

fn get_uri(ws: &Workspace, compilation_unit_name: &str) -> String {
    let root = ws.project_root("semantic-tokens");
    tower_lsp::lsp_types::Url::from_file_path(root.join("src/main/java/foo").join(compilation_unit_name)).unwrap().to_string()
}

fn module_info_uri(ws: &Workspace) -> String {
    let root = ws.project_root("semantic-tokens");
    tower_lsp::lsp_types::Url::from_file_path(root.join("src/main/java/module-info.java")).unwrap().to_string()
}

/// Port of the upstream `TokenAssertionHelper`.
struct TokenAssertionHelper {
    buffer: Vec<String>,
    current_line: usize,
    current_column: usize,
    data: Vec<u64>,
    current_data_index: usize,
    token_type_filter: Vec<&'static str>,
}

impl TokenAssertionHelper {
    fn begin_assertion(ws: &mut Workspace, uri: &str, token_type_filter: &[&'static str]) -> Self {
        let result = ws.request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": uri } }));
        assert!(!result.is_null(), "Provided semantic tokens should not be null");
        let data: Vec<u64> = result["data"]
            .as_array()
            .expect("Semantic tokens data should not be null")
            .iter()
            .map(|v: &Value| v.as_u64().unwrap())
            .collect();
        assert!(data.len() % 5 == 0, "Semantic tokens data should contain 5 integers per token");
        let text = if uri.starts_with("jdt:") {
            ws.request("java/classFileContents", json!({ "uri": uri })).as_str().expect("class file text").to_owned()
        } else {
            ws.read(uri)
        };
        let buffer = text.split('\n').map(|l| l.trim_end_matches('\r').to_owned()).collect();
        TokenAssertionHelper {
            buffer,
            current_line: 0,
            current_column: 0,
            data,
            current_data_index: 0,
            token_type_filter: token_type_filter.to_vec(),
        }
    }

    fn assert_next_token(mut self, expected_text: &str, expected_type: &str, expected_modifiers: &[&str]) -> Self {
        loop {
            assert!(
                self.current_data_index < self.data.len(),
                "Token of type '{expected_type}' should be present in the semantic tokens data"
            );
            let i = self.current_data_index;
            let (delta_line, delta_column, length, type_index, modifiers) =
                (self.data[i] as i64, self.data[i + 1] as i64, self.data[i + 2], self.data[i + 3] as usize, self.data[i + 4]);
            assert!(delta_line >= 0, "Token deltaLine should not be negative");
            assert!(delta_column >= 0, "Token deltaColumn should not be negative");
            assert!(length > 0, "Token length should be greater than zero");
            if delta_line == 0 {
                self.current_column += delta_column as usize;
            } else {
                self.current_line += delta_line as usize;
                self.current_column = delta_column as usize;
            }
            self.current_data_index += 5;

            if self.token_type_filter.is_empty() || self.token_type_filter.contains(&TOKEN_TYPES[type_index]) {
                let line: Vec<u16> = self.buffer[self.current_line].encode_utf16().collect();
                let end = (self.current_column + length as usize).min(line.len());
                let text = String::from_utf16_lossy(&line[self.current_column.min(end)..end]);
                assert_eq!(expected_text, text, "Token text should match the token text range in the buffer.");
                assert_eq!(expected_type, TOKEN_TYPES[type_index], "Token type should be correct. ({expected_text})");
                for (bit, modifier) in TOKEN_MODIFIERS.iter().enumerate() {
                    let encoded = (modifiers >> bit) & 1 == 1;
                    let expected = expected_modifiers.contains(modifier);
                    assert!(
                        expected == encoded,
                        "{} ({expected_text} at {}:{})",
                        if expected {
                            format!("Expected modifier '{modifier}' to be encoded")
                        } else {
                            format!("Did not expect modifier '{modifier}' to be encoded")
                        },
                        self.current_line,
                        self.current_column
                    );
                }
                return self;
            }
        }
    }

    fn end_assertion(self) {
        if self.token_type_filter.is_empty() {
            assert_eq!(self.data.len(), self.current_data_index, "There should be no more tokens");
        } else {
            let mut i = self.current_data_index;
            while i < self.data.len() {
                let current_type = TOKEN_TYPES[self.data[i + 3] as usize];
                assert!(
                    !self.token_type_filter.contains(&current_type),
                    "There should be no more tokens matching the filter, but found '{current_type}' token"
                );
                i += 5;
            }
        }
    }
}

#[test]
fn test_semantic_tokens_source_attachment() {
    let mut ws = setup();
    add_test_library_to_classpath(&mut ws);
    let uri = ws.class_file_uri("semantic-tokens", "foo.bar");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &[])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("bar", "class", &["public", "declaration"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("static", "modifier", &[])
        .assert_next_token("add", "method", &["public", "static", "declaration"])
        .assert_next_token("a", "parameter", &["declaration"])
        .assert_next_token("sum", "variable", &["declaration"])
        .assert_next_token("element", "variable", &["declaration"])
        .assert_next_token("a", "parameter", &[])
        .assert_next_token("sum", "variable", &[])
        .assert_next_token("element", "variable", &[])
        .assert_next_token("sum", "variable", &[])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_methods() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Methods.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["method", "class", "keyword"])
        .assert_next_token("Methods", "class", &["public", "declaration"])
        .assert_next_token("foo1", "method", &["public", "generic", "declaration"])
        .assert_next_token("foo2", "method", &["private", "declaration"])
        .assert_next_token("foo3", "method", &["protected", "declaration"])
        .assert_next_token("foo4", "method", &["static", "declaration"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("foo5", "method", &["native", "declaration"])
        .assert_next_token("foo6", "method", &["deprecated", "declaration"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("main", "method", &["public", "static", "declaration"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("Methods", "class", &["public"])
        .assert_next_token("Methods", "class", &["public", "constructor"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("foo1", "method", &["public", "generic"])
        .assert_next_token("foo2", "method", &["private"])
        .assert_next_token("foo3", "method", &["protected"])
        .assert_next_token("foo4", "method", &["static"])
        .assert_next_token("Integer", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("foo6", "method", &["deprecated"])
        .assert_next_token("foo5", "method", &["native"])
        .assert_next_token("Class", "class", &["readonly", "public", "generic"])
        .assert_next_token("class", "keyword", &[])
        .assert_next_token("m", "class", &[])
        .assert_next_token("Class", "class", &["readonly", "public", "generic"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_constructors() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Constructors.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &[])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("Constructors", "class", &["public", "declaration"])
        .assert_next_token("private", "modifier", &[])
        .assert_next_token("Constructors", "class", &["private", "constructor", "declaration"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("c1", "variable", &["declaration"])
        .assert_next_token("Constructors", "class", &["private", "constructor"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("c2", "variable", &["declaration"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("Constructors", "class", &["private", "constructor"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected"])
        .assert_next_token("i1", "variable", &["declaration"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected", "constructor"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected"])
        .assert_next_token("i2", "variable", &["declaration"])
        .assert_next_token("SomeAnnotation", "annotation", &["public"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected", "constructor"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected", "generic"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("i3", "variable", &["declaration"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerClass", "class", &["protected", "generic", "constructor"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("InnerClass", "class", &["protected", "generic"])
        .assert_next_token("Integer", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("i4", "variable", &["declaration"])
        .assert_next_token("InnerClass", "class", &["protected", "generic", "constructor"])
        .assert_next_token("GenericConstructor", "class", &["private"])
        .assert_next_token("g1", "variable", &["declaration"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("GenericConstructor", "class", &["protected", "generic", "constructor"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerRecord", "record", &["private", "static", "readonly"])
        .assert_next_token("r1", "variable", &["declaration"])
        .assert_next_token("Constructors", "class", &["public"])
        .assert_next_token("InnerRecord", "record", &["private", "constructor"])
        .assert_next_token("InnerRecord", "record", &["protected", "constructor"])
        .assert_next_token("protected", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("InnerClass", "class", &["protected", "generic", "declaration"])
        .assert_next_token("T", "typeParameter", &["declaration"])
        .assert_next_token("private", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("GenericConstructor", "class", &["private", "declaration"])
        .assert_next_token("protected", "modifier", &[])
        .assert_next_token("T", "typeParameter", &["declaration"])
        .assert_next_token("GenericConstructor", "class", &["protected", "generic", "constructor", "declaration"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("InnerEnum", "enum", &["public", "static", "readonly", "declaration"])
        .assert_next_token("FOO", "enumMember", &["public", "static", "readonly", "declaration"])
        .assert_next_token("InnerEnum", "enum", &["private", "constructor", "declaration"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("string", "parameter", &["declaration"])
        .assert_next_token("private", "modifier", &[])
        .assert_next_token("record", "modifier", &[])
        .assert_next_token("InnerRecord", "record", &["private", "static", "readonly", "declaration"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("string", "recordComponent", &["declaration"])
        .assert_next_token("integer", "recordComponent", &["declaration"])
        .assert_next_token("protected", "modifier", &[])
        .assert_next_token("InnerRecord", "record", &["protected", "constructor", "declaration"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("interface", "modifier", &[])
        .assert_next_token("TestInterface", "interface", &["public", "static", "declaration"])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("TestClass", "class", &["declaration"])
        .assert_next_token("implements", "modifier", &[])
        .assert_next_token("TestInterface", "interface", &["public", "static"])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("TestExtends", "class", &["declaration"])
        .assert_next_token("extends", "modifier", &[])
        .assert_next_token("TestClass", "class", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("TestCombo", "class", &["declaration"])
        .assert_next_token("extends", "modifier", &[])
        .assert_next_token("TestClass", "class", &[])
        .assert_next_token("implements", "modifier", &[])
        .assert_next_token("TestInterface", "interface", &["public", "static"])
        .assert_next_token("sealed", "modifier", &[])
        .assert_next_token("interface", "modifier", &[])
        .assert_next_token("TestSealedInterface", "interface", &["sealed", "static", "declaration"])
        .assert_next_token("permits", "modifier", &[])
        .assert_next_token("TestPermits", "class", &["readonly"])
        .assert_next_token("final", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("TestPermits", "class", &["readonly", "declaration"])
        .assert_next_token("implements", "modifier", &[])
        .assert_next_token("TestSealedInterface", "interface", &["sealed", "static"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_fields() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Fields.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["property", "enumMember", "recordComponent"])
        .assert_next_token("bar1", "property", &["public", "declaration"])
        .assert_next_token("bar2", "property", &["private", "declaration"])
        .assert_next_token("bar3", "property", &["protected", "declaration"])
        .assert_next_token("bar2", "property", &["private"])
        .assert_next_token("bar4", "property", &["readonly", "declaration"])
        .assert_next_token("bar5", "property", &["static", "declaration"])
        .assert_next_token("bar6", "property", &["public", "static", "readonly", "declaration"])
        .assert_next_token("FIRST", "enumMember", &["public", "static", "readonly", "declaration"])
        .assert_next_token("SECOND", "enumMember", &["public", "static", "readonly", "declaration"])
        .assert_next_token("i", "recordComponent", &["declaration"])
        .assert_next_token("f", "recordComponent", &["declaration"])
        .assert_next_token("i", "property", &["private", "readonly"])
        .assert_next_token("f", "property", &["private", "readonly"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_variables() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Variables.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["variable", "parameter"])
        .assert_next_token("string", "parameter", &["declaration"])
        .assert_next_token("bar1", "variable", &["declaration"])
        .assert_next_token("string", "parameter", &[])
        .assert_next_token("bar2", "variable", &["declaration"])
        .assert_next_token("bar1", "variable", &[])
        .assert_next_token("bar3", "variable", &["readonly", "declaration"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_types() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Types.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["class", "interface", "enum", "annotation", "record", "typeParameter", "keyword", "modifier"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("Types", "class", &["public", "declaration"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("protected", "modifier", &[])
        .assert_next_token("final", "modifier", &[])
        .assert_next_token("Class", "class", &["public", "readonly", "generic"])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("class", "keyword", &[])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("SomeClass", "class", &["public", "generic"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("SomeClass", "class", &["public", "generic", "typeArgument"])
        .assert_next_token("String", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("Integer", "class", &["public", "readonly", "typeArgument"])
        .assert_next_token("SomeAnnotation", "annotation", &["static"])
        .assert_next_token("Types", "class", &["public"])
        .assert_next_token("class", "keyword", &[])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("SomeClass", "class", &["public", "generic", "declaration"])
        .assert_next_token("T1", "typeParameter", &["declaration"])
        .assert_next_token("T2", "typeParameter", &["declaration"])
        .assert_next_token("T1", "typeParameter", &[])
        .assert_next_token("T2", "typeParameter", &[])
        .assert_next_token("interface", "modifier", &[])
        .assert_next_token("SomeInterface", "interface", &["static", "declaration"])
        .assert_next_token("SomeEnum", "enum", &["static", "readonly", "declaration"])
        .assert_next_token("SomeAnnotation", "annotation", &["static", "declaration"])
        .assert_next_token("Class", "class", &["public", "readonly", "generic"])
        .assert_next_token("record", "modifier", &[])
        .assert_next_token("SomeRecord", "record", &["static", "readonly", "declaration"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_packages() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Packages.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["namespace", "class", "property"])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("lang", "namespace", &["importDeclaration"])
        .assert_next_token("Math", "class", &["public", "readonly", "importDeclaration"])
        .assert_next_token("PI", "property", &["public", "static", "readonly", "importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("util", "namespace", &["importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("NonExistentClass", "namespace", &["importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("nio", "namespace", &["importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("lang", "namespace", &["importDeclaration"])
        .assert_next_token("Math", "class", &["public", "readonly", "importDeclaration"])
        .assert_next_token("java", "namespace", &["importDeclaration"])
        .assert_next_token("lang", "namespace", &["importDeclaration"])
        .assert_next_token("Math", "class", &["public", "readonly", "importDeclaration"])
        .assert_next_token("Packages", "class", &["public", "declaration"])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("lang", "namespace", &[])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("string", "property", &["public", "declaration"])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("lang", "namespace", &[])
        .assert_next_token("String", "class", &["public", "constructor"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_annotations() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Annotations.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["annotation", "annotationMember"])
        .assert_next_token("SomeAnnotation", "annotation", &["public"])
        .assert_next_token("SuppressWarnings", "annotation", &["public"])
        .assert_next_token("SuppressWarnings", "annotation", &["public"])
        .assert_next_token("value", "annotationMember", &["public", "abstract"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_modules() {
    let mut ws = setup();
    let uri = module_info_uri(&ws);
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &[])
        .assert_next_token("@code", "keyword", &["documentation"])
        .assert_next_token("@uses", "keyword", &["documentation"])
        .assert_next_token("@moduleGraph", "keyword", &["documentation"])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("bar", "namespace", &[])
        .assert_next_token("baz", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("base", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("desktop", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("net", "namespace", &[])
        .assert_next_token("http", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("sql", "namespace", &[])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("base", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("desktop", "namespace", &[])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("base", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("net", "namespace", &[])
        .assert_next_token("http", "namespace", &[])
        .assert_next_token("java", "namespace", &[])
        .assert_next_token("sql", "namespace", &[])
        .assert_next_token("Driver", "interface", &["public"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_javadoc() {
    let mut ws = setup();
    let uri = get_uri(&ws, "Javadoc.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &[])
        .assert_next_token("foo", "namespace", &[])
        .assert_next_token("@implNote", "keyword", &["documentation"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("java", "namespace", &["documentation"])
        .assert_next_token("lang", "namespace", &["documentation"])
        .assert_next_token("String", "class", &["public", "readonly", "documentation"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("class", "modifier", &[])
        .assert_next_token("Javadoc", "class", &["public", "declaration"])
        .assert_next_token("@code", "keyword", &["documentation"])
        .assert_next_token("@param", "keyword", &["documentation"])
        .assert_next_token("arg1", "parameter", &["documentation"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("Integer", "class", &["public", "readonly", "documentation"])
        .assert_next_token("@param", "keyword", &["documentation"])
        .assert_next_token("arg2", "parameter", &["documentation"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("Double", "class", &["public", "readonly", "documentation"])
        .assert_next_token("@return", "keyword", &["documentation"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("String", "class", &["public", "readonly", "documentation"])
        .assert_next_token("public", "modifier", &[])
        .assert_next_token("String", "class", &["public", "readonly"])
        .assert_next_token("getString", "method", &["public", "declaration"])
        .assert_next_token("Integer", "class", &["public", "readonly"])
        .assert_next_token("arg1", "parameter", &["declaration"])
        .assert_next_token("Double", "class", &["public", "readonly"])
        .assert_next_token("arg2", "parameter", &["declaration"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("Javadoc", "class", &["public", "documentation"])
        .assert_next_token("getString", "method", &["public", "documentation"])
        .assert_next_token("Integer", "class", &["public", "readonly", "documentation"])
        .assert_next_token("Double", "class", &["public", "readonly", "documentation"])
        .assert_next_token("@see", "keyword", &["documentation"])
        .assert_next_token("getString", "method", &["public", "documentation"])
        .assert_next_token("Integer", "class", &["public", "readonly", "documentation"])
        .assert_next_token("Double", "class", &["public", "readonly", "documentation"])
        .assert_next_token("@return", "keyword", &["documentation"])
        .assert_next_token("@link", "keyword", &["documentation"])
        .assert_next_token("Integer", "class", &["public", "readonly", "documentation"])
        .assert_next_token("private", "modifier", &[])
        .assert_next_token("getInt", "method", &["private", "declaration"])
        .end_assertion();
}

#[test]
fn test_semantic_tokens_class_literals() {
    let mut ws = setup();
    let uri = get_uri(&ws, "ClassLiterals.java");
    TokenAssertionHelper::begin_assertion(&mut ws, &uri, &["keyword"])
        .assert_next_token("class", "keyword", &[])
        .assert_next_token("class", "keyword", &[])
        .end_assertion();
}
