//! Port of `org.eclipse.jdt.ls.core.internal.handlers.HoverHandlerTest`.
//!
//! `Hover.getContents().getLeft()` is the JSON `contents` array: a
//! `MarkedString` (`getRight()`) is `{ "language", "value" }`, a plain string
//! (`getLeft()`) is a JSON string.

mod common;
use common::jdtls::{dos2unix, fixtures_dir, Workspace};
use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

/// `setup()`: imports `eclipse/hello`. The global preference manager of the
/// upstream test supports class file contents (`initPreferenceManager(true)`).
fn setup() -> Workspace {
    let mut ws = Workspace::new();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": true } });
    ws.import_projects(&["eclipse/hello"]);
    ws
}

fn file_uri(ws: &Workspace, project: &str, file: &str) -> String {
    Url::from_file_path(ws.project_root(project).join(file)).unwrap().to_string()
}

/// `createHoverRequest` + `handler.hover`.
fn hover_at(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Value {
    ws.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    )
}

fn contents(hover: &Value) -> Vec<Value> {
    hover["contents"].as_array().cloned().unwrap_or_else(|| panic!("contents is not a list: {hover}"))
}

/// `getRight()`: a MarkedString.
fn right(v: &Value) -> (String, String) {
    assert!(v.is_object(), "expected a MarkedString, got {v}");
    (v["language"].as_str().unwrap().to_owned(), v["value"].as_str().unwrap().to_owned())
}

/// `getLeft()`: a plain string.
fn left(v: &Value) -> String {
    v.as_str().unwrap_or_else(|| panic!("expected a string, got {v}")).to_owned()
}

/// `assertMatches(pattern, value)` (`Pattern.matches`: whole input).
fn assert_matches(pattern: &str, value: &str) {
    let re = regex::Regex::new(&format!("^(?:{pattern})$")).unwrap();
    assert!(re.is_match(value), "{value}\n doesn't match pattern:\n{pattern}");
}

fn get_hover(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> Value {
    hover_at(ws, uri, line, character)
}

fn get_title_hover(ws: &mut Workspace, uri: &str, line: u32, character: u32) -> String {
    let hover = get_hover(ws, uri, line, character);
    assert!(!hover.is_null());
    right(&contents(&hover)[0]).1
}

/// `Paths.get("projects", "maven", "salut", ...)`: a file outside the workspace.
fn standalone_uri(rel: &str) -> String {
    Url::from_file_path(fixtures_dir().join("projects").join(rel)).unwrap().to_string()
}

#[test]
fn test_hover() {
    let mut ws = setup();
    // Hovers on the System.out
    let uri = file_uri(&ws, "hello", "src/java/Foo.java");
    let hover = hover_at(&mut ws, &uri, 5, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let (language, value) = right(&c[0]);
    assert_eq!("java", language, "Unexpected hover {value}");
    assert_eq!("java.Foo", value, "Unexpected hover {value}");
    let doc = left(&c[1]);
    assert_eq!("This is foo", doc, "Unexpected hover {doc}");
}

#[test]
fn test_hover_standalone() {
    let mut ws = setup();
    // Hovers on the System.out
    let uri = standalone_uri("maven/salut/src/main/java/java/Foo.java");
    let hover = hover_at(&mut ws, &uri, 10, 71);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let (language, value) = right(&c[0]);
    assert_eq!("java", language, "Unexpected hover {value}");
    assert_eq!("java.Foo", value, "Unexpected hover {value}");
    let doc = left(&c[1]);
    assert_eq!("This is foo", doc, "Unexpected hover {doc}");
}

#[test]
fn test_hover_package() {
    let mut ws = setup();
    // Hovers on the java.internal package
    let uri = file_uri(&ws, "hello", "src/java/Baz.java");
    let hover = hover_at(&mut ws, &uri, 2, 16);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let signature = right(&c[0]).1;
    assert_eq!("java.internal", signature, "Unexpected signature ");
    let result = left(&c[1]);
    assert_eq!("this is a **bold** package!", result, "Unexpected hover ");
}

#[test]
fn test_empty_hover() {
    let mut ws = setup();
    // Hovers on the System.out
    let uri = standalone_uri("maven/salut/src/main/java/java/Foo.java");
    let hover = hover_at(&mut ws, &uri, 1, 2);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(1, c.len());
    assert_eq!("", left(&c[0]), "Should find empty hover for {uri}");
}

#[test]
fn test_missing_unit() {
    let mut ws = setup();
    let uri = standalone_uri("maven/salut/src/main/java/java/Missing.java");
    let hover = hover_at(&mut ws, &uri, 0, 0);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(1, c.len());
    assert_eq!("", left(&c[0]), "Should find empty hover for {uri}");
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (aspose-words jar from Maven)"]
fn test_invalid_javadoc() {
    let mut ws = setup();
    ws.import_projects(&["maven/aspose"]);
    let uri = ws.class_uri("aspose", "org.sample.TestJavadoc");
    ws.open(&uri);
    let hover = hover_at(&mut ws, &uri, 8, 24);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(2, c.len());
    assert_eq!("com.aspose.words.Document.Document(String fileName) throws Exception", right(&c[0]).1);
    let source = left(&c[1]);
    let prefix = "Source: *[aspose-words-15.12.0-jdk16.jar](jdt://contents/aspose-words-15.12.0-jdk16.jar/com.aspose.words/Document.class?";
    assert!(source.starts_with(prefix), "Unexpected Source from {source}");
    assert!(!source[prefix.len()..].contains('('), "Source URL is not sanitized: {source}");
}

#[test]
fn test_hover_variable() {
    let mut ws = setup();
    // Hover on args parameter
    let uri = file_uri(&ws, "hello", "src/java/Foo.java");
    let hover = hover_at(&mut ws, &uri, 7, 37);
    assert!(!hover.is_null());
    let (language, value) = right(&contents(&hover)[0]);
    assert_eq!("java", language, "Unexpected hover {value}");
    assert_eq!("String[] args - java.Foo.main(String[])", value, "Unexpected hover {value}");
}

#[test]
fn test_hover_method() {
    let mut ws = setup();
    let root = ws.project_root("hello");
    let buf = "package test1;\n\
               import java.util.Vector;\n\
               public class E {\n   public int foo(String s) { }\n   public static void foo2(String s, String s2) { }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", buf);

    assert_eq!("int test1.E.foo(String s)", get_title_hover(&mut ws, &cu, 3, 15));
    assert_eq!("void test1.E.foo2(String s, String s2)", get_title_hover(&mut ws, &cu, 4, 24));
}

#[test]
fn test_hover_type_parameters() {
    let mut ws = setup();
    let root = ws.project_root("hello");
    let buf = "package test1;\n\
               import java.util.Vector;\n\
               public class E<T> {\n   public T foo(T s) { }\n   public <U> U bar(U s) { }\n}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", buf);

    assert_eq!("T", get_title_hover(&mut ws, &cu, 3, 10));
    assert_eq!("T test1.E.foo(T s)", get_title_hover(&mut ws, &cu, 3, 13));
    assert_eq!("<U> U test1.E.bar(U s)", get_title_hover(&mut ws, &cu, 4, 17));
}

#[test]
fn test_hover_inherited_javadoc() {
    let mut ws = setup();
    // Hovers on the overriding foo()
    let uri = file_uri(&ws, "hello", "src/java/Bar.java");
    let hover = hover_at(&mut ws, &uri, 22, 19);
    assert!(!hover.is_null());
    let result = dos2unix(&left(&contents(&hover)[1]));
    let expected = "This method comes from Foo  \n\
                    **Overrides:** foo(...) in Foo\n\
                    \n\
                    * **Parameters:**\n  * **input** an input String";
    assert_eq!(expected, result, "Unexpected hover ");
}

#[test]
fn test_hover_over_null_element() {
    let mut ws = setup();
    let root = ws.project_root("hello");
    let buf = "package test1;\nimport javax.xml.bind.Binder;\npublic class E {}\n";
    let cu = ws.create_cu(&root, "src", "test1", "E.java", buf);
    let hover = get_hover(&mut ws, &cu, 1, 8);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(2, c.len());
    assert_eq!("javax", right(&c[0]).1, "Unexpected hover ");
}

#[test]
#[ignore = "needs Maven dependency download (commons-cli 1.4 is not in the local repository) and jar source attachment"]
fn test_hover_on_package_with_javadoc() {
    let mut ws = setup();
    ws.import_projects(&["maven/salut2"]);
    // Hovers on the org.apache.commons import
    let uri = file_uri(&ws, "salut2", "src/main/java/foo/Bar.java");
    let hover = hover_at(&mut ws, &uri, 2, 22);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let result = right(&c[0]).1;
    assert_eq!("org.apache.commons", result, "Unexpected hover ");

    let source = left(&c[1]);
    // Package source should have no link
    assert_eq!("Source: *commons-cli-1.4.jar*", source, "Unexpected source");
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (hover inside java.lang.Exception's class file)"]
fn test_hover_throwable() {
    let mut ws = setup();
    let uri = "jdt://contents/java.base/java.lang/Exception.class";
    let hover = hover_at(&mut ws, uri, 0, 0);
    assert!(!hover.is_null());
    assert!(!contents(&hover).is_empty(), "Unexpected hover ");
}

#[test]
fn test_hover_unresolved_type() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/unresolvedtype"]);
    // Hovers on the IFoo
    let uri = file_uri(&ws, "unresolvedtype", "src/pckg/Foo.java");
    let hover = hover_at(&mut ws, &uri, 2, 31);
    assert!(!hover.is_null());
    assert!(contents(&hover).is_empty(), "Unexpected hover ");
}

#[test]
#[ignore = "needs attached Javadoc jar support (commons-primitives-1.0-javadoc.jar from Maven)"]
fn test_hover_with_attached_javadoc() {
    let mut ws = setup();
    ws.import_projects(&["maven/attached-javadoc"]);
    let uri = file_uri(&ws, "attached-javadoc", "src/main/java/org/sample/Bar.java");
    let hover = hover_at(&mut ws, &uri, 2, 56);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    assert!(
        content.contains("This class consists exclusively of static methods that operate on or\nreturn ShortCollections"),
        "Unexpected hover :\n{content}"
    );
    assert!(content.contains("**Author:**"), "Unexpected hover :\n{content}");
}

fn salut(ws: &mut Workspace) -> String {
    ws.import_projects(&["maven/salut"]);
    file_uri(ws, "salut", "src/main/java/java/Foo2.java")
}

#[test]
fn test_hover_on_javadoc_with_value_tag() {
    let mut ws = setup();
    let uri = salut(&mut ws);
    let hover = hover_at(&mut ws, &uri, 12, 30);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    assert_matches(r#"\["SimpleStringData"\]\(file:/.*/salut/src/main/java/java/Foo2.java#13\) is a simple String"#, &content);
}

#[test]
fn test_hover_on_javadoc_with_link_to_method_in_class() {
    let mut ws = setup();
    let uri = salut(&mut ws);
    let hover = hover_at(&mut ws, &uri, 18, 25);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    assert_matches(r"\[newMethodBeingLinkedToo\]\(file:/.*/salut/src/main/java/java/Foo2.java#23\)", &content);
}

#[test]
fn test_hover_on_javadoc_with_link_to_method_in_other_class() {
    let mut ws = setup();
    let uri = salut(&mut ws);
    let hover = hover_at(&mut ws, &uri, 29, 25);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    assert_matches(r"\[Foo.linkedFromFoo2\(\)\]\(file:/.*/salut/src/main/java/java/Foo.java#14\)", &content);

    let source = left(&c[2]);
    // Project source should link to project file
    assert_matches(r"Source: \*\[salut\]\(file:/.*/salut/src/main/java/java/Foo2.java#30\)\*", &source);
}

#[test]
fn test_hover_on_javadoc_with_multiple_different_types_of_tags() {
    let mut ws = setup();
    let uri = salut(&mut ws);
    let hover = hover_at(&mut ws, &uri, 44, 25);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    let expected_javadoc = "This Javadoc contains a link to \\[newMethodBeingLinkedToo\\]\\(file:/.*/salut/src/main/java/java/Foo2.java#23\\)\n\
\n\
\\* \\*\\*Parameters:\\*\\*\n  \\* \\*\\*someString\\*\\* the string to enter\n\
\\* \\*\\*Returns:\\*\\*\n  \\* String\n\
\\* \\*\\*Throws:\\*\\*\n  \\* \\[IOException\\]\\(jdt:/.*\\)\n\
\\* \\*\\*Since:\\*\\*\n  \\* 0.0.1\n\
\\* \\*\\*Version:\\*\\*\n  \\* 0.0.1\n\
\\* \\*\\*Author:\\*\\*\n  \\* jpinkney\n\
\\* \\*\\*See Also:\\*\\*\n  \\* \\[Online docs for java\\]\\(https://docs.oracle.com/javase/7/docs/api/\\)\n\
\\* \\*\\*API Note:\\*\\*\n  \\* This is a note";
    assert_matches(expected_javadoc, &dos2unix(&content));
}

#[test]
fn test_hover_when_link_does_not_exist() {
    let mut ws = setup();
    let uri = salut(&mut ws);
    let hover = hover_at(&mut ws, &uri, 51, 26);
    assert!(!hover.is_null(), "Hover is null");
    let c = contents(&hover);
    assert_eq!(3, c.len(), "Unexpected hover contents:\n{hover}");
    let content = left(&c[1]);
    assert_matches("This link doesnt work LinkToSomethingNotFound", &content);
}

#[test]
fn test_hover_javadoc_with_extra_tags() {
    let mut ws = setup();
    let root = ws.project_root("hello");
    let content = "package test1;\n\
/**\n * Some text.\n *\n * @uses java.sql.Driver\n *\n * @moduleGraph\n * @since 9\n */\npublic class Meh {}\n";
    let cu = ws.create_cu(&root, "src", "test1", "Meh.java", content);
    let hover = get_hover(&mut ws, &cu, 9, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "Some text.\n\n* **Since:**\n  * 9\n* **Uses:**\n  * java.sql.Driver\n* **@moduleGraph**";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

fn java_project(ws: &mut Workspace, name: &str) -> std::path::PathBuf {
    ws.import_projects(&[&format!("eclipse/{name}")]);
    ws.project_root(name)
}

#[test]
fn test_hover_javadoc_snippet() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java18");
    let buf = "package test1;\n\
/**\n * A simple program.\n * {@snippet :\n * class HelloWorld {\n *     public static void main(String... args) {\n *         System.out.println(\"Hello World!\");    // @highlight substring=\"println\"\n *     }\n * }\n * }\n */\npublic class Test {\n}\n";
    let cu = ws.create_cu(&root, "src/main/java", "test1", "Test.java", buf);
    let hover = get_hover(&mut ws, &cu, 11, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "A simple program.\n\nclass HelloWorld {  \npublic static void main(String... args) {  \nSystem.out.**println**(\"Hello World!\");    \n}  \n}  \n  \n";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
fn test_hover_javadoc_snippet2() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java18");
    let buf = "package test1;\n/**\n * A simple program.\n * {@snippet :\n *   int x = 1;\n * }\n */\npublic class Test {\n}\n";
    let cu = ws.create_cu(&root, "src/main/java", "test1", "Test.java", buf);
    let hover = get_hover(&mut ws, &cu, 7, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "A simple program.\n\nint x = 1;  \n  \n";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (link to String in the rtstubs.jar test JDK)"]
fn test_hover_javadoc_link_plain() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java18");
    let buf = "package test1;\n/**\n * <h4><a id=\"special_cases_constructor\">Special cases</a></h4>\n * A simple mention of {@linkplain ##special_cases_constructor Special Cases}.\n * <p> A link to {@linkplain String}\n */\npublic class Test {\n}\n";
    let cu = ws.create_cu(&root, "src/main/java", "test1", "Test.java", buf);
    let hover = get_hover(&mut ws, &cu, 6, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "#### Special cases\n\nA simple mention of Special Cases.\n\nA link to [String](jdt://contents/rtstubs.jar/java.lang/String.class)";
    let actual = left(&c[1]);
    // remove everything after the first .class in links, till the first closing parenthesis
    let re = regex::Regex::new(r"(\]\(jdt://[^\)]*?\.class)[^\)]*\)").unwrap();
    let actual = re.replace_all(&actual, "$1)").into_owned();
    let actual = dos2unix(&actual);
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
fn test_hover_javadoc_dl_dt_dd() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java18");
    let buf = concat!(
        "package test1;\n",
        "/**\n",
        " * <dl>\n",
        " *   <dt><a id=\"def_language\"><b>language</b></a></dt>\n",
        " *\n",
        " *   <dd>ISO 639 alpha-2 or alpha-3 language code, or registered\n",
        " *   language subtags up to 8 alpha letters (for future enhancements).\n",
        " *   When a language has both an alpha-2 code and an alpha-3 code, the\n",
        " *   alpha-2 code must be used.  You can find a full list of valid\n",
        " *   language codes in the IANA Language Subtag Registry (search for\n",
        " *   \"Type: language\").  The language field is case insensitive, but\n",
        " *   {@code Locale} always canonicalizes to lower case.</dd>\n",
        " *\n",
        " *   <dd>Well-formed language values have the form\n",
        " *   <code>[a-zA-Z]{2,8}</code>.  Note that this is not the full\n",
        " *   BCP47 language production, since it excludes extlang.  They are\n",
        " *   not needed since modern three-letter language codes replace\n",
        " *   them.</dd>\n",
        " *\n",
        " *   <dd>Example: \"en\" (English), \"ja\" (Japanese), \"kok\" (Konkani)</dd>\n",
        " *\n",
        " *   <dt><a id=\"def_script\"><b>script</b></a></dt>\n",
        " *\n",
        " *   <dd>ISO 15924 alpha-4 script code.  You can find a full list of\n",
        " *   valid script codes in the IANA Language Subtag Registry (search\n",
        " *   for \"Type: script\").  The script field is case insensitive, but\n",
        " *   {@code Locale} always canonicalizes to title case (the first\n",
        " *   letter is upper case and the rest of the letters are lower\n",
        " *   case).</dd>\n",
        " *\n",
        " *   <dd>Well-formed script values have the form\n",
        " *   <code>[a-zA-Z]{4}</code></dd>\n",
        " *\n",
        " *   <dd>Example: \"Latn\" (Latin), \"Cyrl\" (Cyrillic)</dd>\n",
        " *\n",
        " * </dl>\n",
        " */\n",
        "public class Test {\n",
        "}\n",
    );
    let cu = ws.create_cu(&root, "src/main/java", "test1", "Test.java", buf);
    let hover = get_hover(&mut ws, &cu, 37, 15);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "**language**  \n\
ISO 639 alpha-2 or alpha-3 language code, or registered\n    language subtags up to 8 alpha letters (for future enhancements).\n    When a language has both an alpha-2 code and an alpha-3 code, the\n    alpha-2 code must be used. You can find a full list of valid\n    language codes in the IANA Language Subtag Registry (search for\n    \"Type: language\"). The language field is case insensitive, but\n    `Locale` always canonicalizes to lower case.  \n\
Well-formed language values have the form\n    `[a-zA-Z]{2,8}`. Note that this is not the full\n    BCP47 language production, since it excludes extlang. They are\n    not needed since modern three-letter language codes replace\n    them.  \n\
Example: \"en\" (English), \"ja\" (Japanese), \"kok\" (Konkani)  \n\
\n\
**script**  \n\
ISO 15924 alpha-4 script code. You can find a full list of\n    valid script codes in the IANA Language Subtag Registry (search\n    for \"Type: script\"). The script field is case insensitive, but\n    `Locale` always canonicalizes to title case (the first\n    letter is upper case and the rest of the letters are lower\n    case).  \n\
Well-formed script values have the form\n    `[a-zA-Z]{4}`  \n\
Example: \"Latn\" (Latin), \"Cyrl\" (Cyrillic)  ";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
#[ignore = "needs attached Javadoc support (javadoc_location classpath attribute on java-doc-0.0.1-SNAPSHOT.jar)"]
fn test_hover_on_package_with_new_javadoc() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/remote-javadoc"]);
    let uri = file_uri(&ws, "remote-javadoc", "src/main/java/foo/bar/Bar.java");
    let hover = hover_at(&mut ws, &uri, 2, 14);
    assert!(!hover.is_null());
    let javadoc = left(&contents(&hover)[1]);
    assert!(javadoc.contains("this doc is powered by **HTML5**"));
    assert!(!javadoc.contains("----")); // no table nonsense
}

#[test]
#[ignore = "needs jdt:// classfile/source attachment support (JDK 10 source link: Source: *[Java 10](jdt:/...)*)"]
fn test_hover_on_java10var() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/java10"]);
    // Hovers on name.toUpperCase()
    let uri = file_uri(&ws, "java10", "src/main/java/foo/bar/Foo.java");
    let hover = hover_at(&mut ws, &uri, 8, 34);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let javadoc = right(&c[0]).1;
    assert_eq!("String java.lang.String.toUpperCase()", javadoc);

    let source = left(&c[2]);
    // JDK source should link to java file
    assert_matches(r"Source: \*\[Java 10\]\(jdt:/.*\)\*", &source);
}

#[test]
fn test_hover_on_java11var() {
    let mut ws = setup();
    ws.import_projects(&["eclipse/java11"]);
    // Hovers on the 1st var of (var i, var j)
    let uri = file_uri(&ws, "java11", "src/main/java/foo/bar/Foo.java");
    let hover = hover_at(&mut ws, &uri, 15, 21);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let ty = right(&c[0]).1;
    assert_eq!("foo.bar.Foo", ty);
    let javadoc = left(&c[1]);
    assert_eq!("It's a Foo class", javadoc);

    // Hovers on the 2nd var of (var i, var j)
    let hover = hover_at(&mut ws, &uri, 15, 28);
    assert!(!hover.is_null());
    let c = contents(&hover);
    let ty = right(&c[0]).1;
    assert_eq!("foo.bar.Foo.Bar", ty);
    let javadoc = left(&c[1]);
    assert_eq!("It's a Bar interface", javadoc);
}

#[test]
fn test_no_link_when_class_content_unsupported() {
    // initPreferenceManager(false)
    let mut ws = setup();
    ws.init_options = json!({ "extendedClientCapabilities": { "classFileContentsSupport": false } });
    test_class_content_support(&mut ws, "Uses WordUtils");
}

#[test]
#[ignore = "needs Maven dependency download (commons-lang3 3.5 is not in the local repository) and jdt:// classfile support"]
fn test_link_when_class_content_supported() {
    let mut ws = setup();
    test_class_content_support(&mut ws, r"Uses \[WordUtils\]\(jdt:.*\)");
}

fn test_class_content_support(ws: &mut Workspace, expected_javadoc: &str) {
    ws.import_projects(&["maven/salut"]);
    // Hovers on name.toUpperCase()
    let uri = file_uri(ws, "salut", "src/main/java/org/sample/TestJavadoc.java");
    let hover = hover_at(ws, &uri, 17, 20);
    assert!(!hover.is_null());
    let javadoc = left(&contents(&hover)[1]);
    assert_matches(expected_javadoc, &javadoc);
}

// https://github.com/eclipse/eclipse.jdt.ls/issues/1856
#[test]
fn test_enum() {
    let mut ws = setup();
    let uri = file_uri(&ws, "hello", "src/org/sample/TestEnum.java");
    let hover = hover_at(&mut ws, &uri, 5, 33);
    assert!(!hover.is_null());
    let (language, value) = right(&contents(&hover)[0]);
    assert_eq!("java", language, "Unexpected hover {value}");
    assert_eq!("ENUM1", value, "Unexpected hover {value}");
}

#[test]
fn test_hover_markdown_comment() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java23");
    let buf = concat!(
        "package test;\n",
        "/// ## TestClass\n",
        "///\n",
        "/// Paragraph\n",
        "///\n",
        "/// - item 1\n",
        "/// - _item 2_\n",
        "    public class Test {\n",
        "    /// ### m()\n",
        "    ///\n",
        "    /// Paragraph with _emphasis_\n",
        "    /// - item 1\n",
        "    /// - item 2\n",
        "    /// @param i an _integer_ !\n",
        "    void m(int i) {\n",
        "    }\n",
        "}\n",
    );
    let cu = ws.create_cu(&root, "src/main/java", "test", "Test.java", buf);
    let hover = get_hover(&mut ws, &cu, 7, 18);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "## TestClass  \nParagraph  \n- item 1\n- _item 2_";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
fn test_hover_javadoc_disabled() {
    // given - preference is disabled via configuration
    let mut ws = setup();
    ws.settings = json!({ "java": { "hover": { "javadoc": { "enabled": false } } } });

    // when - hover on a class with Javadoc
    let uri = file_uri(&ws, "hello", "src/java/Foo.java");
    let hover_result = hover_at(&mut ws, &uri, 5, 15);

    // then - hover should be null
    assert!(hover_result.is_null(), "Hover should be null when Javadoc is disabled");

    // when - enable preference and hover again
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "java": { "hover": { "javadoc": { "enabled": true } } } } }),
    );
    ws.wait_idle();
    let hover_result_enabled = hover_at(&mut ws, &uri, 5, 15);

    // then - hover should now include Javadoc
    assert!(!hover_result_enabled.is_null());
    let c = contents(&hover_result_enabled);
    assert!(!c.is_empty(), "Hover should contain content when Javadoc is enabled");
    if c.len() > 1 {
        let doc = left(&c[1]);
        assert_eq!("This is foo", doc, "Unexpected hover {doc}");
    }
}

#[test]
fn test_hover_javadoc_with_index_tag() {
    let mut ws = setup();
    let root = ws.project_root("hello");
    let content = "package test1;\n/**\n * Some <dfn>{@index \"locale-sensitive\"}</dfn> text.\n */\npublic class Meh {}\n";
    let cu = ws.create_cu(&root, "src", "test1", "Meh.java", content);
    let hover = get_hover(&mut ws, &cu, 4, 14);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let expected_javadoc = "Some _\"locale-sensitive\"_ text.";
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual, "Unexpected hover ");
}

#[test]
fn test_hover_inline_link_tag_in_markdown_01() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java25");
    let buf = "package test;\n/// {@link #newMethodBeingLinkedToo}\npublic class Markdown{}";
    let cu = ws.create_cu(&root, "src/main/java", "test", "Markdown.java", buf);
    let hover = get_hover(&mut ws, &cu, 2, 14);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let uri = format!("{cu}#3");
    let expected_javadoc = format!("[newMethodBeingLinkedToo]({uri})");
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual.trim_end(), "Unexpected hover ");
}

#[test]
fn test_hover_inline_link_tag_in_markdown_02() {
    let mut ws = setup();
    let root = java_project(&mut ws, "java25");
    let buf = "package test;\n/// {@linkplain #newMethodBeingLinkedToo}\npublic class Markdown{}";
    let cu = ws.create_cu(&root, "src/main/java", "test", "Markdown.java", buf);
    let hover = get_hover(&mut ws, &cu, 2, 14);
    assert!(!hover.is_null());
    let c = contents(&hover);
    assert_eq!(3, c.len());

    let uri = format!("{cu}#3");
    let expected_javadoc = format!("[newMethodBeingLinkedToo]({uri})");
    let actual = dos2unix(&left(&c[1]));
    assert_eq!(expected_javadoc, actual.trim_end(), "Unexpected hover ");
}
