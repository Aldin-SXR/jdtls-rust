//! Public-command coverage beyond the two upstream SmartDetectionHandler tests.
mod common;
use common::jdtls::{test_default_options, Workspace};
use serde_json::{json, Value};

fn setup(level: &str) -> (Workspace, String) {
    setup_versions(level, level)
}
fn setup_versions(source: &str, compliance: &str) -> (Workspace, String) {
    let mut ws = Workspace::new();
    ws.settings = json!({"java": {"edit": {"smartSemicolonDetection": {"enabled": true}}}});
    let mut options = test_default_options();
    for (key, value) in [
        ("source", source),
        ("compliance", compliance),
        ("codegen.targetPlatform", compliance),
    ] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), value.into());
    }
    let root = ws.new_empty_project(&options);
    let uri = ws.create_cu(
        &root,
        "src",
        "p",
        "A.java",
        "package p;\npublic class A {}\n",
    );
    ws.open(&uri);
    ws.request("java/buildWorkspace", json!(false));
    (ws, uri)
}
fn position(text: &str, byte: usize) -> Value {
    let units: Vec<u16> = text[..byte].encode_utf16().collect();
    let (mut line, mut start, mut i) = (0, 0, 0);
    while i < units.len() {
        if units[i] == 13 {
            if units.get(i + 1) == Some(&10) {
                i += 1;
            }
            line += 1;
            start = i + 1;
        } else if units[i] == 10 {
            line += 1;
            start = i + 1;
        }
        i += 1;
    }
    json!({"line": line, "character": units.len() - start})
}
fn detect(ws: &mut Workspace, params: Value) -> Value {
    ws.request(
        "workspace/executeCommand",
        json!({"command": "java.edit.smartSemicolonDetection", "arguments": [params.to_string()]}),
    )
}
// ¦ is the caret; § is the expected destination. No § means null.
fn cases(ws: &mut Workspace, uri: &str, cases: &[&str]) {
    for &marked in cases {
        let marked = format!("package p;\npublic class A {{\n{marked}\n}}\n");
        let clean = marked.replace(['¦', '§'], "");
        let caret_byte = marked[..marked.find('¦').expect("caret")]
            .replace('§', "")
            .len();
        let expected = marked
            .find('§')
            .map(|p| {
                let byte = marked[..p].replace('¦', "").len();
                json!({"uri": uri, "position": position(&clean, byte)})
            })
            .unwrap_or(Value::Null);
        ws.change(uri, &clean);
        ws.request("java/buildWorkspace", json!(false));
        let actual = detect(
            ws,
            json!({"uri": uri, "position": position(&clean, caret_byte)}),
        );
        assert_eq!(actual, expected, "{marked:?}");
    }
}

#[test]
fn trailing_comments_whitespace_and_partition_boundaries() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  int n = ¦1§   // tail",
            "  int n = ¦1§ /* unfinished",
            "  int n = ¦1§ /** unfinished",
            "  int n = ¦1§ /**/",
            "  String s =¦ \"abc\"§/*tail*/",
            "  int n = 1¦/*tail*/",
            "  /* unterminated ¦",
            "  int n = ¦1§ /** doc */   ",
            "  int n = ¦1§ /* block */ // tail",
            "  String s =¦ \"abc\"§  // tail",
            "  char c =¦ 'x'§ /* tail */",
            "  int n = 1¦ // tail",
            "  int n = 1//¦ tail",
            "  /* ¦comment */ int n = 1",
            "  // ¦comment",
            "  /** ¦doc */",
            "  String s = \"¦abc\"",
            "  char c = '¦x'",
            "  ¦   ",
            "  int n = ¦1 /* mid */ + 2§",
        ],
    );
}

#[test]
fn existing_semicolons_across_comments_strings_and_lines() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  int n = ¦1;",
            "  int n = ¦1 ; // tail",
            "  int n = ¦1 // tail\n /* next */ ;",
            "  int n = ¦1\n \"ignored\" 'x' /* ignored */ ;",
            "  int n = ¦1§ // ; ignored\n  int m = 2;",
            "  int n = ¦1§ /* ; */",
            "  int n = ¦1§\n  int m = 2;",
        ],
    );
}

#[test]
fn for_heuristic_and_java_identifier_parts() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  void m() { for (int i = ¦0; i < 2; i++) {} }",
            "  String s =¦ \"for\"",
            "  int n = ¦1 // for",
            "  int before = ¦1§",
            "  int forfeit = ¦1§",
            "  int $for = ¦1§",
            "  int foré = ¦1§",
            "  int for\u{200c} = ¦1§",
            "  int €for = ¦1§",
            "  int n = ¦1§ // before for",
            "  int n = ¦1 // (for)",
        ],
    );
}

#[test]
fn block_and_array_initializer_detection() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  int[] n = { ¦1, 2 }§",
            "  int[] n = new int[] { ¦1, 2 }§",
            "  int[][] n = { { ¦1, 2 } }",
            "  int[] n = /* { */ { ¦1, 2 }§ // }",
            "  int[] n =\n { ¦1, 2 }§",
            "  void m() { ¦int n = 1; }",
            "  void m() ¦{ int n = 1; }§",
            "  void m() {\n    ¦int n = 1; }",
            "  int n = ¦",
            "  Object o = ¦this.",
            "  void m() ¦{",
            "  int n = ¦1 +§",
        ],
    );
}

#[test]
fn method_invocations_and_active_document() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  void m() {\n    String s =¦ String.valueOf(1)§\n  }",
            "  void m() {\n    String s =¦ String.valueOf(\n       1);\n  }",
            "  void m() {\n    String s = String.valueOf(¦1,\n       2);\n  }",
        ],
    );
    let source = "package p;\npublic class A {\n String s = String.valueOf(\n  1);\n}\n";
    ws.change(&uri, source);
    ws.request("java/buildWorkspace", json!(false));
    let params = json!({"uri": uri, "position": {"line": 2, "character": 12}});
    assert!(detect(&mut ws, params.clone()).is_null());
    let root = ws.project_root("TestProject");
    let other = ws.create_cu(
        &root,
        "src",
        "p",
        "B.java",
        "package p; public class B {}\n",
    );
    ws.open(&other);
    ws.request("java/buildWorkspace", json!(false));
    assert_eq!(
        detect(&mut ws, params.clone()),
        json!({"uri": uri, "position": {"line": 2, "character": 27}})
    );
    // A change to A makes it active again, even with both buffers open.
    ws.change(&uri, source);
    ws.request("java/buildWorkspace", json!(false));
    assert!(detect(&mut ws, params).is_null());
}

#[test]
fn text_blocks_strings_escapes_and_unterminated_literals() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  String s =¦ \"text\"§",
            "  String s = ¦\"text\"",
            "  String s =¦ \"a\\\"b\"§ // tail",
            "  String s =¦ \"a\\\\\"§ // tail",
            "  String s =¦ \"unfinished§",
            "  char c =¦ 'unfinished§",
            "  String s =¦ \"\"\"\n body\n \"\"\"",
            "  String s = \"\"\"\n ¦body\n \"\"\"",
            "  String s = \"\"\"\n body\n ¦\"\"\"",
            "  String s = \"\"\"\n body\n \"\"\"¦ + \"x\"§",
            "  String s =¦ \"\"\"invalid same line\"\"\"§",
            "  String s =¦ \"a\\u005C\"b\"§ // tail",
            "  String s =¦ \"a\\u005c\"§ // tail",
        ],
    );
}

#[test]
fn source_level_controls_text_block_partitioning() {
    for (source_level, compliance, expected_body) in
        [("14", "14", true), ("14", "21", true), ("21", "21", false)]
    {
        let (mut ws, uri) = setup_versions(source_level, compliance);
        let source = "package p;\npublic class A {\n String s = \"\"\"\n body\n \"\"\"\n}\n";
        ws.change(&uri, source);
        ws.request("java/buildWorkspace", json!(false));
        let root = ws.project_root("TestProject");
        let other = ws.create_cu(
            &root,
            "src",
            "p",
            "B.java",
            "package p; public class B {}\n",
        );
        ws.open(&other);
        ws.request("java/buildWorkspace", json!(false));
        // WAIT_ACTIVE_ONLY skips the AST of A, exposing the source-level
        // partition rule directly. At 14 the body is ordinary Java text.
        let actual = detect(
            &mut ws,
            json!({"uri": uri, "position": {"line": 3, "character": 1}}),
        );
        let expected = if expected_body {
            json!({"uri": uri, "position": {"line": 3, "character": 5}})
        } else {
            Value::Null
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn unicode_utf16_whitespace_and_java_trim() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  String s = /* 😀 */ ¦\"你好😀\"   ",
            "  int n = ¦1§\u{2000}\u{3000}",
            "  int n = ¦1\u{a0}§",
            "  int n = ¦1\u{85}§",
            "  int n = ¦1§\u{1c}\u{1f}",
            "  int n = ¦1\u{2007}\u{202f}§",
            "  int n = ¦1 /* tail */§\u{2000}",
            "  String s =¦ /* 😀 */ \"你好😀\"§   ",
        ],
    );
}

#[test]
fn markdown_comments_retain_upstream_partition_quirk() {
    let (mut ws, uri) = setup("21");
    cases(
        &mut ws,
        &uri,
        &[
            "  int n = ¦1 /// markdown§",
            "  /// ¦markdown§",
            "  /// ¦markdown§\r  \"more\"",
            "  /// markdown\r  \"¦more\"§",
            "  // ordinary\r  String s =¦ \"more\"§",
            "  /// markdown\r\r  String s =¦ \"more\"§",
            "  int n = ¦1§ // ordinary",
        ],
    );
}

#[test]
fn preference_default_configuration_and_invalid_inputs() {
    let (mut ws, uri) = setup("21");
    let source = "package p;\npublic class A {\n int n = 1\n}\n";
    ws.change(&uri, source);
    ws.request("java/buildWorkspace", json!(false));
    let params = json!({"uri": uri, "position": {"line": 2, "character": 8}});
    assert_eq!(
        detect(&mut ws, params.clone()),
        json!({"uri": uri, "position": {"line": 2, "character": 10}})
    );
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({"settings": {"java.edit.smartSemicolonDetection.enabled": false}}),
    );
    assert!(detect(&mut ws, params.clone()).is_null());
    ws.client().notify(
        "workspace/didChangeConfiguration",
        json!({"settings": {"java.edit.smartSemicolonDetection.enabled": true}}),
    );
    assert!(detect(&mut ws, params.clone()).is_object());
    for invalid in [
        Value::Null,
        json!({}),
        json!({"uri": uri}),
        json!({"uri": null, "position": {"line": 0, "character": 0}}),
        json!({"uri": "not a URI", "position": {"line": 0, "character": 0}}),
        json!({"uri": uri, "position": {"line": 999, "character": 0}}),
        json!({"uri": uri, "position": {"line": 2, "character": 999}}),
    ] {
        assert!(detect(&mut ws, invalid.clone()).is_null(), "{invalid}");
    }
    // The disabled-by-default delegate is a separate freshly initialized server.
    let mut fresh = Workspace::new();
    let root = fresh.new_empty_project(&test_default_options());
    let other = fresh.create_cu(&root, "src", "p", "A.java", source);
    fresh.open(&other);
    assert!(detect(
        &mut fresh,
        json!({"uri": other, "position": {"line": 2, "character": 8}})
    )
    .is_null());
}

#[test]
fn line_delimiters_eof_and_no_mutation() {
    let (mut ws, uri) = setup("21");
    for eol in ["\n", "\r\n", "\r"] {
        let source = format!("package p;{eol}public class A {{{eol} int n = 1 // tail{eol}}}{eol}");
        ws.change(&uri, &source);
        ws.request("java/buildWorkspace", json!(false));
        let params = json!({"uri": uri, "position": {"line": 2, "character": 8}});
        assert_eq!(
            detect(&mut ws, params.clone()),
            json!({"uri": uri, "position": {"line": 2, "character": 10}})
        );
        assert_eq!(
            detect(&mut ws, params),
            json!({"uri": uri, "position": {"line": 2, "character": 10}})
        );
    }
    for source in [
        "package p; class A { int n = 1",
        "package p; class A { String s = \"x\"",
    ] {
        ws.change(&uri, source);
        ws.request("java/buildWorkspace", json!(false));
        assert_eq!(
            detect(
                &mut ws,
                json!({"uri": uri, "position": {"line": 0, "character": 24}})
            ),
            json!({"uri": uri, "position": position(source, source.len())})
        );
        assert!(detect(
            &mut ws,
            json!({"uri": uri, "position": position(source, source.len())})
        )
        .is_null());
    }
    assert_eq!(ws.read(&uri), "package p;\npublic class A {}\n");
}

#[test]
fn character_columns_follow_json_rpc_helpers_without_clamping() {
    let (mut ws, uri) = setup("21");
    let source = "package p;\npublic class A {\n int n = 1\n int m = 2\n}\n";
    ws.change(&uri, source);
    ws.request("java/buildWorkspace", json!(false));
    for input in [
        json!({"line": 3, "character": -4}),
        json!({"line": 1, "character": 20}),
    ] {
        assert_eq!(
            detect(&mut ws, json!({"uri": uri, "position": input})),
            json!({"uri": uri, "position": {"line": 2, "character": 10}})
        );
    }
    assert!(detect(
        &mut ws,
        json!({"uri": uri, "position": {"line": -1, "character": 0}})
    )
    .is_null());
    assert!(detect(
        &mut ws,
        json!({"uri": uri, "position": {"line": 0, "character": -1}})
    )
    .is_null());
}

#[test]
fn virtual_documents_and_raw_models() {
    if common::jdtls::is_oracle() {
        return;
    } // Oracle has no virtual-CU bridge.
    let (mut ws, _) = setup("21");
    let missing = ws.dir.join("missing").join("A.java");
    let missing_uri = tower_lsp::lsp_types::Url::from_file_path(missing).unwrap();
    for uri in [
        "untitled:A.java",
        "untitled:Untitled-1",
        "inmemory://smart/A.java",
        missing_uri.as_str(),
    ] {
        let source = "class A {\n int n = 1\n}\n";
        ws.open_with(uri, source);
        let params = json!({"uri": uri, "position": {"line": 1, "character": 8}});
        let expected = json!({"uri": uri, "position": {"line": 1, "character": 10}});
        assert_eq!(detect(&mut ws, params.clone()), expected);
        assert_eq!(
            ws.request(
                "workspace/executeCommand",
                json!({"command": "java.edit.smartSemicolonDetection", "arguments": [params]})
            ),
            expected
        );
        // The next request must still see the unmodified working copy.
        assert_eq!(
            detect(
                &mut ws,
                json!({"uri": uri, "position": {"line": 1, "character": 10}})
            ),
            Value::Null
        );
    }
}
